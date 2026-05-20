defmodule GameCore.ChunkLifecycle do
  @moduledoc """
  Tracks the interest set and idle countdown that govern a Chunk's hot↔cold
  transitions. Implementation of "Chunk activation" / "Chunk deactivation"
  from `CONTEXT.md`: a Chunk stays hot while any holder pid has expressed
  interest; once the set drops empty, an idle timer arms; if it elapses
  without new interest, the Chunk deactivates.

  ## Calling convention

  Functions in this module are expected to run inside the Chunk's GenServer
  process. `express/2` issues `Process.monitor/1` for the holder pid;
  `release/2` and `handle_down/2` schedule `:idle_check` to the calling
  process via `Process.send_after/3`. Callers from outside the chunk
  process would observe surprising side effects — don't.
  """

  @default_idle_timeout_ms 5_000

  @type t :: %__MODULE__{
          interests: %{pid() => reference()},
          idle_since: nil | integer(),
          idle_timeout_ms: non_neg_integer()
        }

  defstruct interests: %{}, idle_since: nil, idle_timeout_ms: @default_idle_timeout_ms

  @spec new(keyword()) :: t()
  def new(opts \\ []) do
    %__MODULE__{
      interests: %{},
      idle_since: nil,
      idle_timeout_ms: Keyword.get(opts, :idle_timeout_ms, @default_idle_timeout_ms)
    }
  end

  @doc """
  Record that `pid` is interested in keeping the Chunk hot. Monitors `pid`
  so that its exit releases the interest automatically via `handle_down/2`.
  Re-expressing an existing interest is a no-op.
  """
  @spec express(t(), pid()) :: t()
  def express(%__MODULE__{} = lc, pid) when is_pid(pid) do
    interests =
      case Map.fetch(lc.interests, pid) do
        {:ok, _ref} -> lc.interests
        :error -> Map.put(lc.interests, pid, Process.monitor(pid))
      end

    %{lc | interests: interests, idle_since: nil}
  end

  @doc """
  Drop `pid` from the interest set, demonitoring it (with `:flush` so a
  late-arriving DOWN doesn't reach the mailbox). If the set is now empty,
  arms the idle timer.
  """
  @spec release(t(), pid()) :: t()
  def release(%__MODULE__{} = lc, pid) do
    interests =
      case Map.pop(lc.interests, pid) do
        {nil, m} ->
          m

        {ref, m} ->
          Process.demonitor(ref, [:flush])
          m
      end

    maybe_arm_idle(%{lc | interests: interests})
  end

  @doc """
  Process a `:DOWN` for an interested pid. The monitor was already released
  by the BEAM when the message was delivered, so no demonitor is needed.
  """
  @spec handle_down(t(), pid()) :: t()
  def handle_down(%__MODULE__{} = lc, pid) do
    maybe_arm_idle(%{lc | interests: Map.delete(lc.interests, pid)})
  end

  @doc """
  Decide what to do when `:idle_check` fires. Returns `{:deactivate, lc}`
  if the interest set is empty and the idle window has elapsed, otherwise
  `{:keep, lc}`. The lifecycle struct is returned unchanged either way —
  the caller decides whether to stop the GenServer.
  """
  @spec check_idle(t()) :: {:deactivate | :keep, t()}
  def check_idle(%__MODULE__{} = lc) do
    if map_size(lc.interests) == 0 and lc.idle_since != nil and
         System.monotonic_time(:millisecond) - lc.idle_since >= lc.idle_timeout_ms do
      {:deactivate, lc}
    else
      {:keep, lc}
    end
  end

  @doc "Read-only fields exposed via `Chunk.dev_status/1`."
  @spec dev_view(t()) :: %{
          lifecycle: :hot | :idle_armed,
          idle_ms_remaining: nil | non_neg_integer(),
          interest_count: non_neg_integer()
        }
  def dev_view(%__MODULE__{} = lc) do
    lifecycle =
      if map_size(lc.interests) == 0 and lc.idle_since != nil, do: :idle_armed, else: :hot

    idle_ms_remaining =
      case lc.idle_since do
        nil ->
          nil

        ts ->
          elapsed = System.monotonic_time(:millisecond) - ts
          max(lc.idle_timeout_ms - elapsed, 0)
      end

    %{
      lifecycle: lifecycle,
      idle_ms_remaining: idle_ms_remaining,
      interest_count: map_size(lc.interests)
    }
  end

  defp maybe_arm_idle(%__MODULE__{} = lc) do
    cond do
      map_size(lc.interests) > 0 ->
        %{lc | idle_since: nil}

      lc.idle_since == nil ->
        Process.send_after(self(), :idle_check, max(lc.idle_timeout_ms, 1))
        %{lc | idle_since: System.monotonic_time(:millisecond)}

      true ->
        lc
    end
  end
end
