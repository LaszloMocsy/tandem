// Builds the favicons and the Open Graph image from public/logo.svg.
// Run with `bun run assets` after changing the logo, and commit the output.
// The output is committed rather than built on deploy so the OG text renders
// with the same fonts every time.
import { readFile, writeFile } from 'node:fs/promises';
import sharp from 'sharp';

const pub = new URL('../public/', import.meta.url);
const logo = await readFile(new URL('logo.svg', pub));

const png = (size: number) => sharp(logo, { density: 72 * (size / 1024) * 4 }).resize(size, size).png().toBuffer();

// favicon.svg is the logo itself.
await writeFile(new URL('favicon.svg', pub), logo);

await writeFile(new URL('apple-touch-icon.png', pub), await png(180));

// favicon.ico: a single 32x32 PNG wrapped in an ICO container.
const ico32 = await png(32);
const header = Buffer.alloc(22);
header.writeUInt16LE(0, 0); // reserved
header.writeUInt16LE(1, 2); // type: icon
header.writeUInt16LE(1, 4); // image count
header.writeUInt8(32, 6); // width
header.writeUInt8(32, 7); // height
header.writeUInt8(0, 8); // palette size
header.writeUInt8(0, 9); // reserved
header.writeUInt16LE(1, 10); // colour planes
header.writeUInt16LE(32, 12); // bits per pixel
header.writeUInt32LE(ico32.length, 14); // image size
header.writeUInt32LE(22, 18); // image offset
await writeFile(new URL('favicon.ico', pub), Buffer.concat([header, ico32]));

// Open Graph image, 1200x630: logo on the left, headline on the right, on the
// logo's own navy.
const W = 1200;
const H = 630;
const LOGO = 360;
const font = "-apple-system, 'Helvetica Neue', Helvetica, Arial, sans-serif";
const card = `
<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0.4" y2="1">
      <stop offset="0" stop-color="#1b2a6b"/>
      <stop offset="1" stop-color="#0b1233"/>
    </linearGradient>
    <linearGradient id="human" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#ffb47a"/>
      <stop offset="1" stop-color="#ff6b5b"/>
    </linearGradient>
    <linearGradient id="agent" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#7fb2ff"/>
      <stop offset="1" stop-color="#5a5cff"/>
    </linearGradient>
  </defs>
  <rect width="${W}" height="${H}" fill="url(#bg)"/>
  <text x="520" y="200" fill="#ffffff" font-family="${font}" font-size="64" font-weight="700">Tandem</text>
  <text font-family="${font}" font-size="44" font-weight="500" fill="#e6e8eb">
    <tspan x="520" y="290">A task board you share</tspan>
    <tspan x="520" y="348">with your coding agents.</tspan>
  </text>
  <rect x="520" y="400" width="120" height="8" rx="4" fill="url(#human)"/>
  <rect x="652" y="400" width="120" height="8" rx="4" fill="url(#agent)"/>
  <text x="520" y="470" fill="#979ea9" font-family="${font}" font-size="28">tandem.laszlomocsy.com</text>
</svg>`;

const logoPng = await png(LOGO);
await sharp(Buffer.from(card))
  .composite([{ input: logoPng, left: 100, top: Math.round((H - LOGO) / 2) }])
  .png()
  .toFile(new URL('og.png', pub).pathname);

console.log('Wrote favicon.svg, favicon.ico, apple-touch-icon.png and og.png');
