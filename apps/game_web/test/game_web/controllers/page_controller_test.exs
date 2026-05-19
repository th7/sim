defmodule GameWeb.PageControllerTest do
  use GameWeb.ConnCase, async: true

  test "GET / serves the SPA shell with the Vite entry script", %{conn: conn} do
    conn = get(conn, "/")
    body = response(conn, 200)
    assert body =~ ~s(<script type="module" src="/src/main.ts">)
  end
end
