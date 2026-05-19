defmodule GameWeb.Router do
  use GameWeb, :router

  pipeline :api do
    plug :accepts, ["json"]
  end

  scope "/", GameWeb do
    get "/", PageController, :index
  end

  scope "/api", GameWeb do
    pipe_through :api
  end
end
