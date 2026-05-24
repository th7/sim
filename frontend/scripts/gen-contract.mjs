// Generates the frontend's view of the wire contract from the backend export.
//
//   apps/game_web/priv/contract/contract.json   (backend = source of truth)
//        -> frontend/src/contract/types.ts       (TypeScript types, prod-imported)
//        -> frontend/src/contract/contract.json   (verbatim copy, test-imported for ajv)
//
// Run via `npm run gen:contract`. The committed output must stay in sync;
// the consumer suite fails if the copy drifts from the backend export.

import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { compile } from 'json-schema-to-typescript';

const backendUrl = new URL(
  '../../apps/game_web/priv/contract/contract.json',
  import.meta.url,
);
const outDirUrl = new URL('../src/contract/', import.meta.url);

const raw = readFileSync(fileURLToPath(backendUrl), 'utf8');
const contract = JSON.parse(raw);

const pascal = (event) =>
  event.replace(/(^|_)([a-z])/g, (_m, _sep, c) => c.toUpperCase());

const opts = { bannerComment: '', format: true, additionalProperties: false };

const blocks = [];
for (const m of contract.messages) {
  const name = pascal(m.event);
  if (m.payload) blocks.push(await compile(m.payload, `${name}Payload`, opts));
  if (m.reply?.ok) blocks.push(await compile(m.reply.ok, `${name}OkReply`, opts));
  if (m.reply?.error)
    blocks.push(await compile(m.reply.error, `${name}ErrorReply`, opts));
}

const header =
  '// Generated from the backend wire contract — do not edit by hand.\n' +
  '// Run `npm run gen:contract` to regenerate.\n\n';

mkdirSync(fileURLToPath(outDirUrl), { recursive: true });
writeFileSync(fileURLToPath(new URL('types.ts', outDirUrl)), header + blocks.join('\n'));
writeFileSync(fileURLToPath(new URL('contract.json', outDirUrl)), raw);

console.log('wrote src/contract/{types.ts,contract.json}');
