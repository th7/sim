import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import Ajv from 'ajv';

// Consumer verification: the shapes the frontend sends and the mocks it feeds
// itself in contract-style tests must conform to the wire contract the backend
// exports. This is the consumer half of the conformance spine — in particular
// it pins inbound (client→server) payloads, which the backend provider suite
// cannot check because the server never echoes them.

const localUrl = new URL('../src/contract/contract.json', import.meta.url);
const backendUrl = new URL('../../contract/contract.json', import.meta.url);

const contract = JSON.parse(readFileSync(fileURLToPath(localUrl), 'utf8'));
const ajv = new Ajv({ allowUnionTypes: true });

function message(event: string) {
  const m = contract.messages.find((m: { event: string }) => m.event === event);
  if (!m) throw new Error(`no contract message for event: ${event}`);
  return m;
}

const payload = (event: string) => ajv.compile(message(event).payload);
const reply = (event: string, status: string) =>
  ajv.compile(message(event).reply[status]);

describe('wire contract (consumer)', () => {
  it('the committed copy is in sync with the backend export', () => {
    expect(readFileSync(fileURLToPath(localUrl), 'utf8')).toBe(
      readFileSync(fileURLToPath(backendUrl), 'utf8'),
    );
  });

  describe('inbound payloads the client sends', () => {
    it('move conforms', () => expect(payload('move')({ dx: 1, dy: 0 })).toBe(true));
    it('harvest conforms', () =>
      expect(payload('harvest')({ x: 8000, y: 8000 })).toBe(true));
    it('build conforms', () =>
      expect(payload('build')({ type: 'wall', x: 3000, y: 3000 })).toBe(true));
    it('damage conforms', () =>
      expect(payload('damage')({ x: 8000, y: 8000 })).toBe(true));
    it('rejects an unknown build type', () =>
      expect(payload('build')({ type: 'castle', x: 0, y: 0 })).toBe(false));
  });

  describe('outbound mocks the client renders', () => {
    it('snapshot conforms', () =>
      expect(
        payload('snapshot')({
          players: { alice: { x: 8000, y: 8000 } },
          resource_nodes: {
            'tree:8000:8000': { type: 'tree', x: 8000, y: 8000, depleted: false },
          },
          structures: {},
          portals: {},
        }),
      ).toBe(true));
    it('self conforms', () =>
      expect(payload('self')({ inventory: { wood: 5 } })).toBe(true));
    it('relocated conforms', () =>
      expect(
        payload('relocated')({ realm: { kind: 'instance', id: 1 }, coord: [1, 1] }),
      ).toBe(true));
    it('rejects a snapshot with an unannounced field', () =>
      expect(
        payload('snapshot')({
          players: {},
          resource_nodes: {},
          structures: {},
          portals: {},
          extra: 1,
        }),
      ).toBe(false));
  });

  describe('verb replies', () => {
    it('harvest ok conforms', () => expect(reply('harvest', 'ok')({})).toBe(true));
    it('harvest error conforms', () =>
      expect(reply('harvest', 'error')({ reason: 'too_far' })).toBe(true));
    it('rejects an unknown error reason', () =>
      expect(reply('harvest', 'error')({ reason: 'nope' })).toBe(false));
  });
});
