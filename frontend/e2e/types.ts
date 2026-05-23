export type PlayerPos = { x: number; y: number };
export type Coord = [number, number];
export type Inventory = Record<string, number>;
export type ResourceNode = { type: string; x: number; y: number; depleted: boolean };
export type StructureEntry = { x: number; y: number; hp: number; owner: string };
export type PortalEntry = { type: string; direction: string; x: number; y: number };
export type Realm = { kind: 'overworld' } | { kind: 'instance'; id: number };

declare global {
  interface Window {
    __game: {
      username: string;
      homeChunk: Coord;
      players(): Record<string, PlayerPos>;
      inventory(): Inventory;
      structures(): Record<string, StructureEntry>;
      resourceNodes(): Record<string, ResourceNode>;
      portals(): Record<string, PortalEntry>;
      realm(): Realm;
      cameraPos(): { x: number; y: number; z: number };
      click(worldX: number, worldY: number): void;
      harvest(subX: number, subY: number): void;
      build(type: string, subX: number, subY: number): void;
      damage(subX: number, subY: number): void;
    };
  }
}
