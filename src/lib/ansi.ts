/**
 * 极简 ANSI 转义解析器。
 *
 * 云效构建日志里带大量 SGR 颜色码，浏览器不会解释它们，必须先解析成结构化片段。
 * 支持：
 * - SGR 基本色 30-37 / 40-47、亮色 90-97 / 100-107、默认色 39 / 49
 * - 256 色 `38;5;n` / `48;5;n`
 * - 真彩 `38;2;r;g;b` / `48;2;r;g;b`
 * - 粗体 1、暗色 2、斜体 3、下划线 4、反显 7、删除线 9 及对应关闭码
 * - 其它 CSI 控制序列（光标移动、清行）与 OSC 序列直接丢弃
 */

export interface AnsiSpan {
  text: string
  color?: string
  bg?: string
  bold?: boolean
  dim?: boolean
  italic?: boolean
  underline?: boolean
  inverse?: boolean
  strike?: boolean
}

interface SgrState {
  color?: string
  bg?: string
  bold?: boolean
  dim?: boolean
  italic?: boolean
  underline?: boolean
  inverse?: boolean
  strike?: boolean
}

const BASIC_COLORS: Record<number, string> = {
  0: '#000000',
  1: '#c23621',
  2: '#25bc24',
  3: '#adad27',
  4: '#492ee1',
  5: '#d338d3',
  6: '#33bbc8',
  7: '#cbcccd',
}

const BRIGHT_COLORS: Record<number, string> = {
  0: '#818383',
  1: '#fc391f',
  2: '#31e722',
  3: '#eaec23',
  4: '#5935f1',
  5: '#e22ee2',
  6: '#2ee8f0',
  7: '#ffffff',
}

/** xterm 256 色表：0-15 标准色，16-231 6×6×6 立方体，232-255 灰阶。 */
function color256(n: number): string | undefined {
  if (n < 0 || n > 255) return undefined
  if (n < 8) return BASIC_COLORS[n]
  if (n < 16) return BRIGHT_COLORS[n - 8]
  if (n < 232) {
    const idx = n - 16
    const r = Math.floor(idx / 36)
    const g = Math.floor((idx % 36) / 6)
    const b = idx % 6
    const to = (v: number) => (v === 0 ? 0 : 55 + v * 40)
    return `rgb(${to(r)}, ${to(g)}, ${to(b)})`
  }
  const gray = 8 + (n - 232) * 10
  return `rgb(${gray}, ${gray}, ${gray})`
}

function applyCode(state: SgrState, codes: number[]): void {
  if (codes.length === 0) codes = [0]
  for (let i = 0; i < codes.length; i++) {
    const c = codes[i]
    switch (c) {
      case 0:
        state.color = undefined
        state.bg = undefined
        state.bold = false
        state.dim = false
        state.italic = false
        state.underline = false
        state.inverse = false
        state.strike = false
        break
      case 1:
        state.bold = true
        break
      case 2:
        state.dim = true
        break
      case 3:
        state.italic = true
        break
      case 4:
        state.underline = true
        break
      case 7:
        state.inverse = true
        break
      case 9:
        state.strike = true
        break
      case 22:
        state.bold = false
        state.dim = false
        break
      case 23:
        state.italic = false
        break
      case 24:
        state.underline = false
        break
      case 27:
        state.inverse = false
        break
      case 29:
        state.strike = false
        break
      case 39:
        state.color = undefined
        break
      case 49:
        state.bg = undefined
        break
      case 38:
      case 48: {
        const isFg = c === 38
        const mode = codes[i + 1]
        if (mode === 5) {
          const col = color256(codes[i + 2] ?? 0)
          if (isFg) state.color = col
          else state.bg = col
          i += 2
        } else if (mode === 2) {
          const col = `rgb(${codes[i + 2] ?? 0}, ${codes[i + 3] ?? 0}, ${codes[i + 4] ?? 0})`
          if (isFg) state.color = col
          else state.bg = col
          i += 4
        }
        break
      }
      default:
        if (c >= 30 && c <= 37) state.color = BASIC_COLORS[c - 30]
        else if (c >= 40 && c <= 47) state.bg = BASIC_COLORS[c - 40]
        else if (c >= 90 && c <= 97) state.color = BRIGHT_COLORS[c - 90]
        else if (c >= 100 && c <= 107) state.bg = BRIGHT_COLORS[c - 100]
        break
    }
  }
}

const ESC = '\u001b'

/** 把一行日志解析成带样式的片段；无转义码时原样返回单片段。 */
export function parseAnsiLine(line: string): AnsiSpan[] {
  if (!line.includes(ESC) && !line.includes('\u009b')) {
    return [{ text: line }]
  }

  const spans: AnsiSpan[] = []
  const state: SgrState = {}
  let buf = ''
  let i = 0

  const flush = () => {
    if (buf.length === 0) return
    spans.push({
      text: buf,
      color: state.color,
      bg: state.bg,
      bold: state.bold || undefined,
      dim: state.dim || undefined,
      italic: state.italic || undefined,
      underline: state.underline || undefined,
      inverse: state.inverse || undefined,
      strike: state.strike || undefined,
    })
    buf = ''
  }

  while (i < line.length) {
    const ch = line[i]
    if (ch === ESC || ch === '\u009b') {
      // CSI: ESC [ ... <final byte 0x40-0x7E>
      if (line[i + 1] === '[' || ch === '\u009b') {
        let j = ch === '\u009b' ? i + 1 : i + 2
        let params = ''
        while (j < line.length) {
          const code = line.charCodeAt(j)
          if (code >= 0x40 && code <= 0x7e) break
          params += line[j]
          j++
        }
        const final = line[j] ?? ''
        if (final === 'm') {
          flush()
          const codes = params
            .split(/[;]/)
            .filter((s) => s.length > 0)
            .map((s) => Number.parseInt(s, 10))
            .filter((n) => !Number.isNaN(n))
          applyCode(state, codes)
        }
        // 其它 CSI 序列（光标移动 / 清行等）直接丢弃
        i = j + 1
        continue
      }
      // OSC: ESC ] ... (BEL | ST)
      if (line[i + 1] === ']') {
        let j = i + 2
        while (j < line.length && line[j] !== '\u0007' && !(line[j] === ESC && line[j + 1] === '\\')) {
          j++
        }
        i = line[j] === '\u0007' ? j + 1 : j + 2
        continue
      }
      // 其它两字节转义，跳过
      i += 2
      continue
    }
    buf += ch
    i++
  }
  flush()
  return spans
}

/** 去掉一行里的所有 ANSI 转义码，用于复制 / 下载纯文本。 */
export function stripAnsi(text: string): string {
  // eslint-disable-next-line no-control-regex
  return text
    .replace(/\u001b\[[0-9;?]*[ -/]*[@-~]/g, '')
    .replace(/\u001b\][^\u0007\u001b]*(\u0007|\u001b\\)/g, '')
    .replace(/[\u0000-\u0008\u000b-\u001f]/g, '')
}

/** 把片段状态转成可直接绑定的 style 对象。 */
export function spanStyle(span: AnsiSpan): Record<string, string> {
  const style: Record<string, string> = {}
  if (span.color) style.color = span.color
  if (span.bg) style['background-color'] = span.bg
  if (span.bold) style['font-weight'] = '700'
  if (span.dim) style.opacity = '0.65'
  if (span.italic) style['font-style'] = 'italic'
  const deco: string[] = []
  if (span.underline) deco.push('underline')
  if (span.strike) deco.push('line-through')
  if (deco.length) style['text-decoration'] = deco.join(' ')
  return style
}
