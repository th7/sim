defmodule GameWeb.ContractProviderTest do
  @moduledoc """
  Provider verification: the payloads real channels actually push and reply
  with must conform to the exported wire contract (`GameWeb.Contract`). This is
  the conformance spine — declared contract vs. the real wire — that keeps the
  schema from silently drifting from what the server emits.

  Inbound verb *request* payloads (move/harvest/build/damage) aren't checked
  here — the server never echoes them; they're verified on the consumer
  (frontend) side. This suite covers outbound pushes, verb replies, join
  replies, and the freshness of the committed artifact.
  """
  use GameWeb.ChannelCase

  import GameWeb.ContractAssertions
  import GameWeb.ChunkCleanup, only: [reset_chunks_and_instances: 1]

  # Chunks are global GenServers; tear them down between tests so leftover
  # entities/structures (e.g. a wall built by an earlier test) can't leak.
  setup :reset_chunks_and_instances

  # Join the snapshot ChunkChannel (which starts the chunk) then the
  # PlayerChannel (which places the Player's entity in that now-hot chunk).
  # Order matters: the Session only places the entity if the chunk is started.
  defp connect(prefix) do
    username = "#{prefix}-#{System.unique_integer([:positive])}"
    sock = socket(GameWeb.UserSocket, "provider:#{username}", %{})
    {:ok, _, _chunk} = subscribe_and_join(sock, GameWeb.ChunkChannel, "chunk:0:0", %{})

    {:ok, _, player} =
      subscribe_and_join(sock, GameWeb.PlayerChannel, "player:#{username}", %{
        "username" => username,
        "initial_chunk" => [0, 0]
      })

    {sock, player, username}
  end

  describe "snapshot push" do
    test "a real snapshot conforms to the snapshot contract" do
      {:ok, _reply, _socket} =
        GameWeb.UserSocket
        |> socket("provider:snapshot", %{})
        |> subscribe_and_join(GameWeb.ChunkChannel, "chunk:0:0", %{})

      assert_push "snapshot", snap, 2_000
      assert_conforms("snapshot", :payload, snap)
    end
  end

  describe "harvest reply" do
    test "harvesting the tree at spawn replies :ok, conforming to the contract" do
      {_sock, player, _username} = connect("harvester")
      # The Player spawns on the central tree at chunk-(0,0) centre (8_000, 8_000).
      ref = push(player, "harvest", %{"x" => 8_000, "y" => 8_000})
      assert_reply ref, :ok, reply, 2_000
      assert_reply_conforms("harvest", :ok, reply)
    end

    test "an out-of-range harvest replies :error, conforming to the contract" do
      {_sock, player, _username} = connect("harvester")
      ref = push(player, "harvest", %{"x" => 999_999, "y" => 999_999})
      assert_reply ref, :error, reply, 2_000
      assert_reply_conforms("harvest", :error, reply)
      assert reply.reason == "too_far"
    end
  end

  describe "build reply" do
    test "building a wall with enough wood replies :ok, conforming to the contract" do
      {_sock, player, username} = connect("builder")
      # (3_000, 3_000) is clear of the 5-tree spawn cluster and inside chunk (0,0).
      GameCore.Chunk.set_inventory(GameCore.Chunks.whereis(:overworld, {0, 0}), username, %{
        wood: 5
      })

      ref = push(player, "build", %{"type" => "wall", "x" => 3_000, "y" => 3_000})
      assert_reply ref, :ok, reply, 2_000
      assert_reply_conforms("build", :ok, reply)
    end

    test "building without enough wood replies :error, conforming to the contract" do
      {_sock, player, _username} = connect("builder")
      ref = push(player, "build", %{"type" => "wall", "x" => 3_000, "y" => 3_000})
      assert_reply ref, :error, reply, 2_000
      assert_reply_conforms("build", :error, reply)
      assert reply.reason == "insufficient_materials"
    end

    test "building an unknown type replies :error invalid_type, conforming to the contract" do
      {_sock, player, _username} = connect("builder")
      ref = push(player, "build", %{"type" => "castle", "x" => 3_000, "y" => 3_000})
      assert_reply ref, :error, reply, 2_000
      assert_reply_conforms("build", :error, reply)
      assert reply.reason == "invalid_type"
    end
  end

  describe "damage reply" do
    test "damaging out of range replies :error too_far, conforming to the contract" do
      {_sock, player, _username} = connect("smasher")
      ref = push(player, "damage", %{"x" => 999_999, "y" => 999_999})
      assert_reply ref, :error, reply, 2_000
      assert_reply_conforms("damage", :error, reply)
      assert reply.reason == "too_far"
    end

    test "damaging an in-range cell with no structure replies :error no_target, conforming to the contract" do
      {_sock, player, _username} = connect("smasher")
      # (8_000, 8_000) holds a tree (a Resource node), not a Structure.
      ref = push(player, "damage", %{"x" => 8_000, "y" => 8_000})
      assert_reply ref, :error, reply, 2_000
      assert_reply_conforms("damage", :error, reply)
      assert reply.reason == "no_target"
    end
  end

  describe "self push" do
    test "the self push after a harvest conforms to the contract" do
      {_sock, player, _username} = connect("self")
      # Harvesting the spawn tree drains a yield into the Inventory and pushes
      # the owner's updated `self` view.
      push(player, "harvest", %{"x" => 8_000, "y" => 8_000})

      assert_push "self", payload, 2_000
      assert_conforms("self", :payload, payload)
    end
  end

  describe "relocated push" do
    test "the relocated push on Instance entry conforms to the contract" do
      {_sock, player, _username} = connect("wanderer")

      # The into_instance Portal sits at (4_000, 4_000); the SW diagonal from
      # the spawn (8_000, 8_000) runs straight through it. Holding SW slides the
      # Player out of the spawn cluster and onto the Portal, triggering Instance
      # entry — a realm transition, which is what publishes `relocated`.
      push(player, "move", %{"dx" => -1, "dy" => -1})

      assert_push "relocated", payload, 15_000
      assert_conforms("relocated", :payload, payload)
    end
  end

  describe "stats push" do
    test "the dev stats push conforms to the contract" do
      {sock, _player, username} = connect("watcher")

      # A live session gives the overlay a hot centre chunk surrounded by cold
      # neighbours, exercising both `around` entry shapes.
      {:ok, _, _stats} =
        subscribe_and_join(sock, GameWeb.DevStatsChannel, "dev:stats", %{"username" => username})

      assert_push "stats", payload, 2_000
      assert_conforms("stats", :payload, payload)
    end
  end

  describe "join error replies" do
    setup do
      %{sock: socket(GameWeb.UserSocket, "provider:join", %{})}
    end

    test "a username/topic mismatch is rejected, conforming to the contract", %{sock: sock} do
      assert {:error, reply} =
               subscribe_and_join(sock, GameWeb.PlayerChannel, "player:alice", %{
                 "username" => "bob"
               })

      assert_reply_conforms("join", :error, reply)
      assert reply.reason == "username_mismatch"
    end

    test "a malformed instance topic is rejected, conforming to the contract", %{sock: sock} do
      assert {:error, reply} =
               subscribe_and_join(sock, GameWeb.ChunkChannel, "instance:nope", %{})

      assert_reply_conforms("join", :error, reply)
      assert reply.reason == "bad_topic"
    end

    test "a non-integer chunk coordinate is rejected, conforming to the contract", %{sock: sock} do
      assert {:error, reply} =
               subscribe_and_join(sock, GameWeb.ChunkChannel, "chunk:abc:def", %{})

      assert_reply_conforms("join", :error, reply)
      assert reply.reason == "unavailable"
    end
  end

  describe "contract export" do
    test "the committed contract.json is up to date with the contract module" do
      committed =
        [__DIR__, "..", "priv", "contract", "contract.json"]
        |> Path.join()
        |> File.read!()

      assert committed == GameWeb.Contract.to_json(),
             "priv/contract/contract.json is stale — run `mix contract.export`"
    end
  end
end
