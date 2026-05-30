# Source: design/economy.md (gather), design/glossary.md (Resource node, Gatherable, ItemStack, Inventory, Item, Footprint)
Feature: Harvest a Resource node
  As a Player
  I want to harvest Resource nodes for materials
  So that I can draw materials out of the world into my Inventory.

  Scenario: Harvesting a tree yields wood into the Inventory
    Given a Player next to an un-depleted Resource node (a tree)
    When the Player harvests the node
    Then wood ItemStacks are added to the Player's Inventory

  Scenario: A harvested node depletes
    Given a Player harvesting a Resource node
    When the node has been harvested to depletion
    Then the node yields no further materials until it respawns

  Scenario: A depleted node respawns on a timer
    Given a Resource node that has been depleted
    When its respawn timer elapses
    Then the node is harvestable again

  Scenario: A node's Footprint is unchanged by depletion
    Given an un-depleted Resource node with a Footprint
    When the node becomes depleted
    Then its Footprint is identical to when it was full
