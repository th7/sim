const PHX_HTTP = process.env.PHX_HTTP ?? 'http://localhost:4000';

export const PHX_WS = PHX_HTTP.replace(/^http/, 'ws') + '/socket';

export async function assertServerUp(): Promise<void> {
  try {
    const r = await fetch(PHX_HTTP);
    if (!r.ok) throw new Error(`status ${r.status}`);
  } catch (err) {
    throw new Error(
      `Phoenix server not reachable at ${PHX_HTTP}. Start it with \`mix phx.server\`. (${(err as Error).message})`,
    );
  }
}

export function uniqUsername(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 8)}`;
}
