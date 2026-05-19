defmodule GameWeb.PageController do
  use GameWeb, :controller

  @spa_shell """
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
    html(conn, @spa_shell)
  end
end
