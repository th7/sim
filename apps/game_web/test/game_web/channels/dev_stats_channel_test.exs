defmodule GameWeb.DevStatsChannelTest do
  use GameWeb.ChannelCase, async: false

  setup do
    on_exit(fn ->
      # Sessions and Chunks each register monitored entries in their Registry
      # via names. Process exit triggers an async DOWN that drops the entry.
      # Tear children down explicitly and then wait for both registries to
      # clear so the next test's `start_supervised!(... name: via(...))`
      # doesn't collide.
      for {_, pid, _, _} <- DynamicSupervisor.which_children(GameCore.SessionSupervisor) do
        DynamicSupervisor.terminate_child(GameCore.SessionSupervisor, pid)
      end

      for {_, pid, _, _} <- DynamicSupervisor.which_children(GameCore.ChunkSupervisor) do
        DynamicSupervisor.terminate_child(GameCore.ChunkSupervisor, pid)
      end

      Stream.repeatedly(fn ->
        Registry.count(GameCore.Sessions) + Registry.count(GameCore.Chunks)
      end)
      |> Enum.find(fn count -> count == 0 end)
    end)

    :ok
  end

  defp uniq(prefix), do: "#{prefix}-#{System.unique_integer([:positive])}"

  defp start_session!(username, initial_chunk, opts) do
    {:ok, pid} =
      GameCore.start_session(
        Keyword.merge([username: username, initial_chunk: initial_chunk], opts)
      )

    _ = :sys.get_state(pid)
    pid
  end

  defp join_dev(username) do
    GameWeb.UserSocket
    |> socket("user_" <> username, %{})
    |> subscribe_and_join(GameWeb.DevStatsChannel, "dev:stats", %{"username" => username})
  end

  test "joining dev:stats receives a stats push with global counts" do
    {:ok, _reply, _socket} = join_dev(uniq("dev"))

    assert_push "stats", payload
    assert is_integer(payload.active_chunks)
    assert is_integer(payload.total_players)
    assert payload.active_chunks >= 0
    assert payload.total_players >= 0
  end

  test "stats push includes a 7x7 `around` list keyed to the joiner's current chunk" do
    alice = uniq("alice")
    _ = start_session!(alice, {0, 0}, warm_radius: 2)

    {:ok, _reply, _socket} = join_dev(alice)

    assert_push "stats", payload
    assert is_list(payload.around)
    assert length(payload.around) == 49

    coords = Enum.map(payload.around, fn entry -> {entry.cx, entry.cy} end)

    expected =
      for cx <- -3..3, cy <- -3..3, do: {cx, cy}

    assert Enum.sort(coords) == Enum.sort(expected)

    by_coord = Map.new(payload.around, fn entry -> {{entry.cx, entry.cy}, entry} end)

    # The 5x5 warm set around (0,0) should be :hot.
    for cx <- -2..2, cy <- -2..2 do
      entry = Map.fetch!(by_coord, {cx, cy})
      assert entry.lifecycle == :hot, "expected (#{cx},#{cy}) to be :hot, got #{entry.lifecycle}"
    end

    # The outer 7x7 ring (chunks outside the warm set) should be :cold.
    outer = for cx <- -3..3, cy <- -3..3, max(abs(cx), abs(cy)) == 3, do: {cx, cy}

    for coord <- outer do
      entry = Map.fetch!(by_coord, coord)
      assert entry.lifecycle == :cold
    end
  end

  test "chunks that have lost all interests appear as :idle_armed with a remaining countdown" do
    alice = uniq("alice")
    bob = uniq("bob")

    _ = start_session!(alice, {0, 0}, warm_radius: 2)

    # Bob's only warm chunk is (3, 0) — outside Alice's warm set, inside her 7x7.
    bob_pid = start_session!(bob, {3, 0}, warm_radius: 0)

    bob_ref = Process.monitor(bob_pid)
    :ok = DynamicSupervisor.terminate_child(GameCore.SessionSupervisor, bob_pid)
    assert_receive {:DOWN, ^bob_ref, :process, ^bob_pid, _}

    # Let the chunk process the release_interest call.
    case GameCore.Chunks.whereis(:overworld, {3, 0}) do
      pid when is_pid(pid) -> _ = :sys.get_state(pid)
      _ -> :ok
    end

    {:ok, _reply, _socket} = join_dev(alice)

    assert_push "stats", payload
    entry = Enum.find(payload.around, fn e -> {e.cx, e.cy} == {3, 0} end)

    assert entry.lifecycle == :idle_armed
    assert is_integer(entry.idle_ms_remaining)
    assert entry.idle_ms_remaining > 0
  end
end
