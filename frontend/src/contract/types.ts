// Generated from the backend wire contract — do not edit by hand.
// Run `npm run gen:contract` to regenerate.

export interface BuildPayload {
  type: "wall";
  x: number;
  y: number;
}

export interface BuildOkReply {}

export interface BuildErrorReply {
  reason:
    | "invalid_type"
    | "out_of_chunk"
    | "footprint_blocked"
    | "no_player"
    | "insufficient_materials"
    | "no_build_in_instance"
    | "no_chunk";
}

export interface DamagePayload {
  x: number;
  y: number;
}

export interface DamageOkReply {}

export interface DamageErrorReply {
  reason: "no_player" | "too_far" | "no_target" | "no_chunk";
}

export interface HarvestPayload {
  x: number;
  y: number;
}

export interface HarvestOkReply {}

export interface HarvestErrorReply {
  reason: "no_player" | "too_far" | "depleted" | "no_target" | "no_chunk";
}

export interface JoinOkReply {}

export interface JoinErrorReply {
  reason: "username_mismatch" | "bad_topic" | "unavailable";
}

export interface MovePayload {
  dx: number;
  dy: number;
}

export interface RelocatedPayload {
  /**
   * @minItems 2
   * @maxItems 2
   */
  coord: [number, number];
  realm:
    | {
        kind: "overworld";
      }
    | {
        id: number;
        kind: "instance";
      };
}

export interface SelfPayload {
  inventory: {
    [k: string]: number;
  };
}

export interface SnapshotPayload {
  players: {
    [k: string]: {
      x: number;
      y: number;
    };
  };
  portals: {
    [k: string]: {
      direction: string;
      type: string;
      x: number;
      y: number;
    };
  };
  resource_nodes: {
    [k: string]: {
      depleted: boolean;
      type: string;
      x: number;
      y: number;
    };
  };
  structures: {
    [k: string]: {
      hp: number;
      owner: string;
      type: string;
      x: number;
      y: number;
    };
  };
}

export interface StatsPayload {
  active_chunks: number;
  around: {
    cx: number;
    cy: number;
    entity_count: number;
    idle_ms_remaining: number | null;
    lifecycle: "hot" | "idle_armed" | "cold";
  }[];
  total_players: number;
}
