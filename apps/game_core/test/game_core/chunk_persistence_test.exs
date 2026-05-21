defmodule GameCore.ChunkPersistenceTest do
  use ExUnit.Case, async: false

  alias GameCore.Chunk

  defmodule StubRepo do
    @moduledoc false
    @behaviour GameCore.ChunkRepo

    def start_link(players \\ %{}) do
      Agent.start_link(fn -> %{players: players, flushes: []} end, name: __MODULE__)
    end

    def put_player(p), do: Agent.update(__MODULE__, &put_in(&1.players[p.username], p))
    def flushes, do: Agent.get(__MODULE__, & &1.flushes)

    @impl true
    def fetch_player(username), do: Agent.get(__MODULE__, & &1.players[username])

    @impl true
    def flush_players(coord, players) do
      Agent.update(__MODULE__, &%{&1 | flushes: &1.flushes ++ [{coord, players}]})
      :ok
    end

    @impl true
    def build_structure(_coord, _owner, _type, _x, _y, _inv),
      do: {:error, :not_supported_in_this_stub}

    @impl true
    def destroy_structure(_id), do: :ok

    @impl true
    def fetch_structures(_coord), do: []
  end

  setup do
    {:ok, _pid} = StubRepo.start_link()
    on_exit(fn -> if Process.whereis(StubRepo), do: Agent.stop(StubRepo) end)
    :ok
  end

  test "join hydrates the Player's saved position from the repo" do
    StubRepo.put_player(%{username: "alice", chunk_x: 0, chunk_y: 0, x: 5_000, y: 3_000})

    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, repo: StubRepo}
      )

    :ok = Chunk.join(chunk, "alice")

    assert %{players: %{"alice" => %{x: 5_000, y: 3_000}}} = Chunk.snapshot(chunk)
  end

  test "join puts a brand-new Player at the chunk's center" do
    chunk =
      start_supervised!(
        {Chunk, coord: {2, -1}, auto_tick: false, auto_flush: false, repo: StubRepo}
      )

    :ok = Chunk.join(chunk, "newbie")

    # chunk (2,-1) center in sub-units: (2*16000 + 8000, -1*16000 + 8000) = (40000, -8000)
    assert %{players: %{"newbie" => %{x: 40_000, y: -8_000}}} = Chunk.snapshot(chunk)
  end

  test "leave flushes the Player's last position to the repo" do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, repo: StubRepo}
      )

    :ok = Chunk.join(chunk, "alice")
    %{players: %{"alice" => %{x: x0}}} = Chunk.snapshot(chunk)
    :ok = Chunk.set_intent(chunk, "alice", {1.0, 0.0})
    send(chunk, :tick)
    _ = :sys.get_state(chunk)
    :ok = Chunk.leave(chunk, "alice")

    assert [{{0, 0}, [%{username: "alice", x: x, y: _}]}] = StubRepo.flushes()
    assert x > x0
  end

  test "periodic flush sends all current Players to the repo" do
    chunk =
      start_supervised!(
        {Chunk, coord: {2, -1}, auto_tick: false, auto_flush: false, repo: StubRepo}
      )

    :ok = Chunk.join(chunk, "alice")
    :ok = Chunk.join(chunk, "bob")
    send(chunk, :flush_db)
    _ = :sys.get_state(chunk)

    assert [{{2, -1}, players}] = StubRepo.flushes()
    usernames = players |> Enum.map(& &1.username) |> Enum.sort()
    assert usernames == ["alice", "bob"]
  end
end
