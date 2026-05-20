Elixir umbrella for a real-time multiplayer game. Gameplay (ECS, chunks-as-processes) lives in `game_core`; persistence in `game_persistence`; Phoenix Channels in `game_web`. No LiveView.

## Project guidelines

- Prefer obvious tests and obvious code over documentation. When documentation is unavoidable, keep it terse.
- Run `mix precommit` before considering work done.
- Use `Req` for HTTP — not `:httpoison`, `:tesla`, or `:httpc`.

## Elixir guidelines

- Lists don't support `mylist[i]`; use `Enum.at/2`, pattern matching, or `List`.
- Block expressions return values — bind the result; don't rebind a variable inside the block.
- Don't nest multiple modules in one file.
- Structs don't implement Access — use `struct.field`, not `struct[:field]`.
- Don't `String.to_atom/1` on user input (memory leak).
- Predicates end in `?`; reserve `is_*` for guards.
- `DynamicSupervisor` and `Registry` children need `name:` in the child spec.
- For concurrent enumeration with back-pressure, use `Task.async_stream/3` with `timeout: :infinity`.

## Test guidelines

- Start processes with `start_supervised!/1` for automatic cleanup.
- No `Process.sleep/1` or `Process.alive?/1` — wait via `Process.monitor/1` + `assert_receive {:DOWN, ...}`. To flush a GenServer's mailbox before the next call, use `_ = :sys.get_state(pid)`.
