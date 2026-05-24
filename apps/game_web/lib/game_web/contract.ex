defmodule GameWeb.Contract do
  @moduledoc """
  The wire contract between the Phoenix Channels backend and the Three.js
  frontend, declared as data. Each message's payload (and, for inbound verbs,
  its reply) is a plain JSON Schema map — consumed directly by validators and
  serialized verbatim by `mix contract.export`.

  Strict everywhere (`additionalProperties: false`): an unannounced field on
  the wire is a contract breach, caught by the provider-verification suite.
  """

  @contract_path Path.expand(
                   Path.join([__DIR__, "..", "..", "priv", "contract", "contract.json"])
                 )

  @doc """
  The catalogue of wire messages, each tagged with its direction, owning topic
  family, and whether it carries a payload / a reply. The schemas themselves
  live in `payload_schema/1` and `reply_schema/1`.
  """
  def messages do
    [
      %{event: "move", dir: :in, topic: :player, payload: true, reply: false},
      %{event: "harvest", dir: :in, topic: :player, payload: true, reply: true},
      %{event: "build", dir: :in, topic: :player, payload: true, reply: true},
      %{event: "damage", dir: :in, topic: :player, payload: true, reply: true},
      %{event: "snapshot", dir: :out, topic: :chunk, payload: true, reply: false},
      %{event: "self", dir: :out, topic: :player, payload: true, reply: false},
      %{event: "relocated", dir: :out, topic: :player, payload: true, reply: false},
      %{event: "stats", dir: :out, topic: :dev, payload: true, reply: false},
      %{event: "join", dir: :in, topic: :all, payload: false, reply: true}
    ]
  end

  @doc "Absolute path of the committed JSON artifact."
  def export_path, do: @contract_path

  @doc """
  The full wire contract as deterministic, pretty-printed JSON: every message
  with its direction, topic, payload schema, and (for verbs/joins) reply
  schema. This is what `mix contract.export` writes and the frontend imports.
  """
  def to_json do
    messages()
    |> Enum.sort_by(& &1.event)
    |> Enum.map(&describe_message/1)
    |> then(&%{"messages" => &1})
    |> deep_sort()
    |> Jason.encode!(pretty: true)
    |> Kernel.<>("\n")
  end

  defp describe_message(%{event: event} = m) do
    %{"event" => event, "direction" => to_string(m.dir), "topic" => to_string(m.topic)}
    |> maybe_put("payload", m.payload && payload_schema(event))
    |> maybe_put("reply", m.reply && reply_schema(event))
  end

  defp maybe_put(map, _key, falsey) when falsey in [nil, false], do: map
  defp maybe_put(map, key, value), do: Map.put(map, key, value)

  # Recursively sort object keys so the serialized artifact is byte-stable
  # regardless of map iteration order — the freshness test depends on it.
  defp deep_sort(map) when is_map(map) and not is_struct(map) do
    map
    |> Enum.sort_by(fn {k, _} -> to_string(k) end)
    |> Enum.map(fn {k, v} -> {to_string(k), deep_sort(v)} end)
    |> Jason.OrderedObject.new()
  end

  defp deep_sort(list) when is_list(list), do: Enum.map(list, &deep_sort/1)
  defp deep_sort(other), do: other

  @doc """
  JSON Schema for the payload of an outbound push or inbound verb.

  Inbound (client→server) verb payloads are part of the contract but aren't
  provider-verifiable — the server never echoes them. They're verified on the
  consumer side (the frontend's contract tests).
  """
  def payload_schema("move"), do: object(%{"dx" => number(), "dy" => number()})
  def payload_schema("harvest"), do: object(%{"x" => int(), "y" => int()})
  def payload_schema("build"), do: object(%{"type" => enum(["wall"]), "x" => int(), "y" => int()})
  def payload_schema("damage"), do: object(%{"x" => int(), "y" => int()})

  def payload_schema("snapshot") do
    object(%{
      "players" => map_of(object(%{"x" => int(), "y" => int()})),
      "resource_nodes" =>
        map_of(object(%{"type" => str(), "x" => int(), "y" => int(), "depleted" => bool()})),
      "structures" =>
        map_of(
          object(%{
            "type" => str(),
            "x" => int(),
            "y" => int(),
            "hp" => int(),
            "owner" => str()
          })
        ),
      "portals" =>
        map_of(object(%{"type" => str(), "direction" => str(), "x" => int(), "y" => int()}))
    })
  end

  def payload_schema("self") do
    object(%{"inventory" => map_of(int())})
  end

  def payload_schema("relocated") do
    object(%{"realm" => realm_schema(), "coord" => coord_schema()})
  end

  def payload_schema("stats") do
    object(%{
      "active_chunks" => int(),
      "total_players" => int(),
      "around" =>
        array_of(
          object(%{
            "cx" => int(),
            "cy" => int(),
            "lifecycle" => enum(~w(hot idle_armed cold)),
            "idle_ms_remaining" => nullable_int(),
            "entity_count" => int()
          })
        )
    })
  end

  @doc """
  JSON Schema for an inbound verb's reply, keyed by Phoenix reply status
  (`"ok"` / `"error"`). A successful reply carries an empty payload; an error
  reply carries a `reason` drawn from the verb's enumerated failure set.
  """
  def reply_schema("harvest") do
    %{
      "ok" => object(%{}),
      "error" => object(%{"reason" => enum(~w(no_player too_far depleted no_target no_chunk))})
    }
  end

  def reply_schema("build") do
    %{
      "ok" => object(%{}),
      "error" =>
        object(%{
          "reason" =>
            enum(
              ~w(invalid_type out_of_chunk footprint_blocked no_player insufficient_materials no_build_in_instance no_chunk)
            )
        })
    }
  end

  def reply_schema("damage") do
    %{
      "ok" => object(%{}),
      "error" => object(%{"reason" => enum(~w(no_player too_far no_target no_chunk))})
    }
  end

  # Channel join: an empty reply on success, or a reason on rejection. Applies
  # to every channel; the reason set is the union across all join handlers.
  def reply_schema("join") do
    %{
      "ok" => object(%{}),
      "error" => object(%{"reason" => enum(~w(username_mismatch bad_topic unavailable))})
    }
  end

  # --- JSON Schema constructors. No DSL semantics — these build plain JSON
  # --- Schema maps that ex_json_schema validates and Jason serializes as-is.

  # An object with a fixed, fully-required, closed set of properties.
  defp object(properties) do
    %{
      "type" => "object",
      "additionalProperties" => false,
      "required" => properties |> Map.keys() |> Enum.sort(),
      "properties" => properties
    }
  end

  # An object keyed by arbitrary strings whose values all match `value_schema`
  # (e.g. username -> player, wire_id -> resource node).
  defp map_of(value_schema) do
    %{"type" => "object", "additionalProperties" => value_schema}
  end

  defp array_of(item_schema), do: %{"type" => "array", "items" => item_schema}

  defp int, do: %{"type" => "integer"}
  defp nullable_int, do: %{"type" => ["integer", "null"]}
  defp number, do: %{"type" => "number"}
  defp str, do: %{"type" => "string"}
  defp bool, do: %{"type" => "boolean"}
  defp enum(values), do: %{"type" => "string", "enum" => values}

  # A `[cx, cy]` chunk coordinate.
  defp coord_schema do
    %{"type" => "array", "items" => int(), "minItems" => 2, "maxItems" => 2}
  end

  # The realm a Player is in: the shared Overworld or a numbered Instance.
  defp realm_schema do
    %{
      "oneOf" => [
        object(%{"kind" => enum(["overworld"])}),
        object(%{"kind" => enum(["instance"]), "id" => int()})
      ]
    }
  end
end
