#!/usr/bin/env node
/**
 * 生成应用图标源文件 src-tauri/app-icon.png（1024×1024 RGBA）。
 *
 * 纯 Node 实现，零第三方依赖：手写 PNG 编码（IHDR + IDAT(zlib) + IEND）。
 * 生成后请执行 `npm run icon:build`，由 `tauri icon` 派生出
 * .ico / .icns / 各尺寸 png，供打包使用。
 */
import { deflateSync } from 'node:zlib'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const SIZE = 1024
const here = dirname(fileURLToPath(import.meta.url))
const OUT = resolve(here, '../src-tauri/app-icon.png')

// ---------------------------------------------------------------------------
// PNG 编码
// ---------------------------------------------------------------------------
const CRC_TABLE = (() => {
  const t = new Int32Array(256)
  for (let n = 0; n < 256; n++) {
    let c = n
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    t[n] = c
  }
  return t
})()

function crc32(buf) {
  let c = 0xffffffff
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8)
  return (c ^ 0xffffffff) >>> 0
}

function chunk(type, data) {
  const len = Buffer.alloc(4)
  len.writeUInt32BE(data.length, 0)
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data])
  const crc = Buffer.alloc(4)
  crc.writeUInt32BE(crc32(body), 0)
  return Buffer.concat([len, body, crc])
}

function encodePng(width, height, rgba) {
  const stride = width * 4
  const raw = Buffer.alloc((stride + 1) * height)
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0 // filter type: None
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride)
  }

  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(width, 0)
  ihdr.writeUInt32BE(height, 4)
  ihdr[8] = 8 // bit depth
  ihdr[9] = 6 // color type: RGBA
  ihdr[10] = 0 // compression
  ihdr[11] = 0 // filter
  ihdr[12] = 0 // interlace

  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ])
}

// ---------------------------------------------------------------------------
// 绘制
// ---------------------------------------------------------------------------
const MARGIN = 56
const RADIUS = 190
const TOP = [37, 99, 235] // #2563eb
const BOTTOM = [124, 58, 237] // #7c3aed

/** 点到线段的最短距离 */
function distToSegment(px, py, ax, ay, bx, by) {
  const dx = bx - ax
  const dy = by - ay
  const len2 = dx * dx + dy * dy
  let t = len2 === 0 ? 0 : ((px - ax) * dx + (py - ay) * dy) / len2
  t = Math.max(0, Math.min(1, t))
  const cx = ax + t * dx
  const cy = ay + t * dy
  return Math.hypot(px - cx, py - cy)
}

function inRoundedRect(x, y) {
  const x0 = MARGIN
  const y0 = MARGIN
  const x1 = SIZE - MARGIN
  const y1 = SIZE - MARGIN
  if (x < x0 || x > x1 || y < y0 || y > y1) return false
  const cx = Math.max(x0 + RADIUS, Math.min(x, x1 - RADIUS))
  const cy = Math.max(y0 + RADIUS, Math.min(y, y1 - RADIUS))
  return Math.hypot(x - cx, y - cy) <= RADIUS
}

// 三个节点 + 两段连线，构成「流水线」意象
const NODES = [
  { x: 296, y: 690, r: 74 },
  { x: 512, y: 372, r: 74 },
  { x: 728, y: 690, r: 74 },
]
const EDGES = [
  [NODES[0], NODES[1]],
  [NODES[1], NODES[2]],
]
const LINE_W = 30
const RING_INNER = 34

const pixels = Buffer.alloc(SIZE * SIZE * 4)

for (let y = 0; y < SIZE; y++) {
  const t = (y - MARGIN) / (SIZE - MARGIN * 2)
  const tt = Math.max(0, Math.min(1, t))
  for (let x = 0; x < SIZE; x++) {
    const o = (y * SIZE + x) * 4
    if (!inRoundedRect(x + 0.5, y + 0.5)) {
      pixels[o + 3] = 0
      continue
    }

    // 背景渐变
    let r = TOP[0] + (BOTTOM[0] - TOP[0]) * tt
    let g = TOP[1] + (BOTTOM[1] - TOP[1]) * tt
    let b = TOP[2] + (BOTTOM[2] - TOP[2]) * tt

    // 右上角柔光
    const glow = Math.max(0, 1 - Math.hypot(x - SIZE * 0.82, y - SIZE * 0.16) / 620)
    r += (255 - r) * glow * 0.22
    g += (255 - g) * glow * 0.22
    b += (255 - b) * glow * 0.22

    // 连线（白色实心）
    for (const [a, c] of EDGES) {
      if (distToSegment(x + 0.5, y + 0.5, a.x, a.y, c.x, c.y) <= LINE_W / 2) {
        r = g = b = 255
        break
      }
    }

    // 节点：白色圆环
    for (const n of NODES) {
      const d = Math.hypot(x + 0.5 - n.x, y + 0.5 - n.y)
      if (d <= n.r && d >= RING_INNER) {
        r = g = b = 255
        break
      }
      // 环心填深色，增强对比
      if (d < RING_INNER) {
        r *= 0.55
        g *= 0.55
        b *= 0.6
        break
      }
    }

    pixels[o] = Math.round(Math.max(0, Math.min(255, r)))
    pixels[o + 1] = Math.round(Math.max(0, Math.min(255, g)))
    pixels[o + 2] = Math.round(Math.max(0, Math.min(255, b)))
    pixels[o + 3] = 255
  }
}

mkdirSync(dirname(OUT), { recursive: true })
writeFileSync(OUT, encodePng(SIZE, SIZE, pixels))
console.log(`已生成图标源文件：${OUT}（${SIZE}×${SIZE} RGBA PNG）`)
console.log('下一步执行：npm run icon:build  （由 tauri icon 派生 .ico / .icns / 各尺寸 png）')
