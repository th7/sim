export type PlayerPos = { x: number; y: number };
export type Coord = [number, number];

declare global {
  interface Window {
    __game: {
      username: string;
      homeChunk: Coord;
      players(): Record<string, PlayerPos>;
    };
  }
}
