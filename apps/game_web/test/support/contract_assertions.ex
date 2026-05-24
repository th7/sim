defmodule GameWeb.ContractAssertions do
  @moduledoc """
  Test-only helpers asserting that real channel payloads conform to the wire
  contract (`GameWeb.Contract`). Validation is deliberately test-time only
  (see PLAN.md) — `ex_json_schema` is a `:test`-env dependency and never ships
  in the running server.
  """
  import ExUnit.Assertions

  @doc """
  Asserts an outbound push (or inbound verb) payload conforms to the contract
  schema for `event`.
  """
  def assert_conforms(event, :payload, data) do
    validate!("#{event} payload", GameWeb.Contract.payload_schema(event), data)
  end

  @doc """
  Asserts a verb's reply payload conforms to the contract schema for that
  `event` and Phoenix reply `status` (`:ok` / `:error`).
  """
  def assert_reply_conforms(event, status, data) do
    schema = GameWeb.Contract.reply_schema(event) |> Map.fetch!(to_string(status))
    validate!("#{event} #{status} reply", schema, data)
  end

  # Validates the JSON-serialized form — exactly what the client receives over
  # the wire (string keys, atoms encoded) — not the raw Elixir map.
  defp validate!(label, schema, data) do
    json = data |> Jason.encode!() |> Jason.decode!()
    resolved = ExJsonSchema.Schema.resolve(schema)

    case ExJsonSchema.Validator.validate(resolved, json) do
      :ok ->
        :ok

      {:error, errors} ->
        flunk("""
        #{label} does not conform to the wire contract:

        #{Enum.map_join(errors, "\n", fn {msg, path} -> "  #{path}: #{msg}" end)}

        payload (JSON form):
        #{inspect(json, pretty: true)}
        """)
    end
  end
end
