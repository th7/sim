defmodule GameWeb.PageController do
  use GameWeb, :controller

  # Dev hits Vite on :3000, which serves its own index.html with /src/main.ts
  # and HMR. Any env without Vite (e2e, prod) hits Phoenix directly: serve
  # the Vite-built index.html from priv/static, which references the hashed
  # bundle that Plug.Static handles. Tests force the dev shell via
  # `config :game_web, :force_dev_shell, true` so they exercise the dev path
  # regardless of whether a prior `mix assets.deploy` left a built file
  # behind in priv/static.
  @dev_shell """
  <!DOCTYPE html>
  <html lang="en">
    <head>
      <meta charset="UTF-8" />
      <title>sim</title>
    </head>
    <body>
      <div id="app"></div>
      <script type="module" src="/src/main.ts"></script>
    </body>
  </html>
  """

  def index(conn, _params) do
    cond do
      Application.get_env(:game_web, :force_dev_shell, false) ->
        html(conn, @dev_shell)

      true ->
        built = Path.join(:code.priv_dir(:game_web), "static/index.html")

        if File.exists?(built) do
          conn
          |> put_resp_content_type("text/html")
          |> send_file(200, built)
        else
          html(conn, @dev_shell)
        end
    end
  end
end
