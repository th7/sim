defmodule GameCore.WarmSet do
  @moduledoc """
  The set of Chunks a connected Player's session keeps hot on their behalf
  (see `CONTEXT.md`). One WarmSet per Session: bound to a `holder` pid (the
  Session) at construction, centered on the Player's current Chunk, sized
  by a radius.

  ## Synchronous activation

  `new/3` activates every Chunk in the initial window and expresses interest
  on the holder's behalf **before returning**. Callers depending on this
  property — notably `GameCore.Session.init/1`, which warms before
  `start_link/1` returns so a fast-terminating channel can't race the
  warm-up — get it for free.
  """

  alias GameCore.{Chunk, ChunkGeometry, Chunks}

  @default_radius 2

  @type t :: %__MODULE__{
          center: Chunk.coord(),
          radius: non_neg_integer(),
          members: MapSet.t(Chunk.coord()),
          holder: pid(),
          repo: module()
        }

  defstruct [:center, :radius, :members, :holder, :repo]

  @doc """
  Build a new WarmSet centered on `center`, held by `holder`. Synchronously
  activates every Chunk in the initial window and expresses interest.
  """
  @spec new(Chunk.coord(), pid(), keyword()) :: t()
  def new(center, holder, opts \\ []) when is_pid(holder) do
    ws = %__MODULE__{
      center: center,
      radius: Keyword.get(opts, :radius, @default_radius),
      members: MapSet.new(),
      holder: holder,
      repo: Keyword.get(opts, :repo, GameCore.ChunkRepo.Null)
    }

    recenter(ws, center)
  end

  @doc "The set of Chunk coords currently kept hot by this WarmSet."
  @spec members(t()) :: MapSet.t(Chunk.coord())
  def members(%__MODULE__{members: members}), do: members

  @doc """
  Shift the warm window to a new center. Activates any Chunks newly in the
  window, releases interest on any that just fell out. Returns the updated
  WarmSet.
  """
  @spec recenter(t(), Chunk.coord()) :: t()
  def recenter(%__MODULE__{} = ws, new_center) do
    want = ChunkGeometry.neighborhood(new_center, ws.radius)
    Enum.each(MapSet.difference(want, ws.members), &warm_up(&1, ws))
    Enum.each(MapSet.difference(ws.members, want), &cool_down(&1, ws))
    %{ws | center: new_center, members: want}
  end

  @doc """
  Release interest on every member Chunk and clear the set. Use on session
  teardown; once-released Chunks will deactivate after their idle window
  unless some other holder has them.
  """
  @spec release_all(t()) :: t()
  def release_all(%__MODULE__{} = ws) do
    Enum.each(ws.members, &cool_down(&1, ws))
    %{ws | members: MapSet.new()}
  end

  defp warm_up(coord, ws), do: warm_up(coord, ws, 2)
  defp warm_up(_coord, _ws, 0), do: :ok

  defp warm_up(coord, ws, retries) do
    {:ok, pid} = Chunks.ensure_started(coord, ws.repo)

    try do
      Chunk.express_interest(pid, ws.holder)
    catch
      :exit, _ -> warm_up(coord, ws, retries - 1)
    end
  end

  defp cool_down(coord, ws) do
    case Chunks.whereis(coord) do
      pid when is_pid(pid) -> safe(fn -> Chunk.release_interest(pid, ws.holder) end)
      _ -> :ok
    end
  end

  defp safe(fun) do
    fun.()
  catch
    _, _ -> :ok
  end
end
