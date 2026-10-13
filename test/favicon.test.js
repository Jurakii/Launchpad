const test = require('node:test');
const assert = require('node:assert');
const { normalizeUrl, findIconLinks, decodeIco } = require('../lib/favicon');

test('normalizeUrl accepts bare hosts and http(s) URLs only', () => {
  assert.strictEqual(normalizeUrl('example.com/page').href, 'https://example.com/page');
  assert.strictEqual(normalizeUrl(' http://example.org ').href, 'http://example.org/');
  assert.strictEqual(normalizeUrl('localhost:3000').href, 'https://localhost:3000/');
  assert.strictEqual(normalizeUrl('steam://rungameid/440'), null);
  assert.strictEqual(normalizeUrl('mailto:someone@example.com'), null);
  assert.strictEqual(normalizeUrl(''), null);
});

test('findIconLinks ranks by size, resolves URLs and skips SVG/mask icons', () => {
  const html = `
    <head>
      <link rel="stylesheet" href="/site.css">
      <link rel="icon" href="/favicon-32.png" sizes="32x32">
      <link rel="mask-icon" href="/mask.svg" color="#000">
      <link rel="icon" type="image/svg+xml" href="/icon.svg">
      <link rel="apple-touch-icon" href="touch.png">
      <LINK REL='shortcut icon' HREF='/fav.ico?v=1&amp;x=2'>
      <link rel="icon" href="/favicon-32.png" sizes="32x32">
      <link rel="icon" href="javascript:alert(1)">
      <link rel="icon" sizes="16x16 192x192" href="//cdn.example.net/big.png">
    </head>`;
  assert.deepStrictEqual(findIconLinks(html, 'https://example.com/dir/page'), [
    'https://cdn.example.net/big.png',
    'https://example.com/dir/touch.png',
    'https://example.com/favicon-32.png',
    'https://example.com/fav.ico?v=1&x=2',
  ]);
});

test('findIconLinks honours <base href>', () => {
  const html = '<base href="https://static.example.com/assets/"><link rel="icon" href="fav.png">';
  assert.deepStrictEqual(findIconLinks(html, 'https://example.com/'), ['https://static.example.com/assets/fav.png']);
});

// --- ICO fixtures ---

function ico(entries) {
  const header = Buffer.alloc(6 + entries.length * 16);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(entries.length, 4);
  let offset = header.length;
  entries.forEach(({ width, bpp, data }, i) => {
    const o = 6 + i * 16;
    header[o] = width === 256 ? 0 : width;
    header[o + 1] = width === 256 ? 0 : width;
    header.writeUInt16LE(1, o + 4);
    header.writeUInt16LE(bpp, o + 6);
    header.writeUInt32LE(data.length, o + 8);
    header.writeUInt32LE(offset, o + 12);
    offset += data.length;
  });
  return Buffer.concat([header, ...entries.map((e) => e.data)]);
}

function dibHeader(width, height, bpp) {
  const h = Buffer.alloc(40);
  h.writeUInt32LE(40, 0);
  h.writeInt32LE(width, 4);
  h.writeInt32LE(height * 2, 8); // ICO DIBs count the AND mask in the height
  h.writeUInt16LE(1, 12);
  h.writeUInt16LE(bpp, 14);
  return h;
}

const pixels = (buf) => Array.from({ length: buf.length / 4 }, (_, i) => [...buf.subarray(i * 4, i * 4 + 4)]);

test('decodeIco reads 32bpp BMP entries top-down with premultiplied alpha', () => {
  const data = Buffer.concat([
    dibHeader(2, 2, 32),
    Buffer.from([0, 0, 255, 255, 0, 255, 0, 128]), // bottom row: red, half-transparent green
    Buffer.from([255, 0, 0, 255, 0, 0, 0, 0]), // top row: blue, transparent
    Buffer.alloc(8), // AND mask, unused when alpha is present
  ]);
  const out = decodeIco(ico([{ width: 2, bpp: 32, data }]));
  assert.strictEqual(out.width, 2);
  assert.strictEqual(out.height, 2);
  assert.deepStrictEqual(pixels(out.bitmap), [
    [255, 0, 0, 255],
    [0, 0, 0, 0],
    [0, 0, 255, 255],
    [0, 128, 0, 128],
  ]);
});

test('decodeIco reads paletted entries and applies the AND mask', () => {
  const palette = Buffer.alloc(16 * 4);
  palette.set([10, 20, 30, 0], 1 * 4);
  palette.set([40, 50, 60, 0], 2 * 4);
  const data = Buffer.concat([
    dibHeader(2, 2, 4),
    palette,
    Buffer.from([0x12, 0, 0, 0]), // bottom row: index 1, index 2
    Buffer.from([0x21, 0, 0, 0]), // top row: index 2, index 1
    Buffer.from([0b01000000, 0, 0, 0]), // bottom row mask: second pixel transparent
    Buffer.from([0, 0, 0, 0]),
  ]);
  const out = decodeIco(ico([{ width: 2, bpp: 4, data }]));
  assert.deepStrictEqual(pixels(out.bitmap), [
    [40, 50, 60, 255],
    [10, 20, 30, 255],
    [10, 20, 30, 255],
    [0, 0, 0, 0],
  ]);
});

test('decodeIco picks the largest entry and passes embedded PNG through', () => {
  const png = Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), Buffer.from('rest')]);
  const small = Buffer.concat([dibHeader(1, 1, 32), Buffer.alloc(4 + 4)]);
  const out = decodeIco(
    ico([
      { width: 1, bpp: 32, data: small },
      { width: 256, bpp: 32, data: png },
    ])
  );
  assert.ok(out.png.equals(png));
});

test('decodeIco rejects non-ICO and truncated data', () => {
  assert.strictEqual(decodeIco(Buffer.from('<html>not an icon</html>')), null);
  const truncated = ico([{ width: 2, bpp: 32, data: dibHeader(2, 2, 32) }]); // no pixel rows
  assert.strictEqual(decodeIco(truncated), null);
});
