defmodule GameCore.Structure.Catalogue do
  @moduledoc """
  Closed-enum catalogue of Structure types. Each entry declares its build
  cost (a list of `{Item.t, count}` deducted on placement) and starting HP.
  """

  alias GameCore.Item

  @type type :: :wall
  @type cost :: [{Item.t(), pos_integer()}]

  @walls_cost [{:wood, 5}]
  @walls_max_hp 100
  @walls_footprint %{shape: :aabb, w: 1_000, h: 1_000}

  @spec types() :: [type()]
  def types, do: [:wall]

  @spec valid?(any()) :: boolean()
  def valid?(t), do: t in types()

  @spec cost(type()) :: cost()
  def cost(:wall), do: @walls_cost

  @spec max_hp(type()) :: pos_integer()
  def max_hp(:wall), do: @walls_max_hp

  @spec footprint(type()) :: GameCore.Components.Footprint.t()
  def footprint(:wall), do: @walls_footprint
end
