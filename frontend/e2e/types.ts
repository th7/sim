export type PlayerPos = { x: number; y: number };
export type Coord = [number, number];
export type Inventory = Record<string, number>;
export type ResourceNode = { type: string; x: number; y: number; depleted: boolean };
export type StructureEntry = { x: number; y: number; hp: number; owner: string };

declare global {
  interface Window {
    __game: {
      username: string;
      homeChunk: Coord;
      players(): Record<string, PlayerPos>;
      inventory(): Inventory;
      structures(): Record<string, StructureEntry>;
      resourceNodes(): Record<string, ResourceNode>;
      click(worldX: number, worldY: number): void;
      harvest(subX: number, subY: number): void;
      build(type: string, subX: number, subY: number): void;
      damage(subX: number, subY: number): void;
    };
  }
}
