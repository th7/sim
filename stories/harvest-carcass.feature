# Source: design/living-world.md (the carcass), design/economy.md (Gatherable),
#         design/glossary.md (Carcass, Gatherable, meat, hide, ItemStack, Inventory)
Feature: Harvest a Carcass
  As a Player
  I want to harvest the Carcass of a killed animal
  So that hunting feeds the material economy.

  Scenario: A killed animal leaves a Carcass
    Given an animal is killed
    Then a Carcass remains where it died

  Scenario: A Player harvests a Carcass for meat and hide
    Given a Player next to a Carcass
    When the Player harvests the Carcass
    Then meat and hide ItemStacks are added to the Player's Inventory

  Scenario: A Carcass perishes if left
    Given a Carcass that is not fully harvested or consumed
    When enough time passes
    Then the Carcass perishes and can no longer be harvested

  Scenario: A Carcass is contested by NPCs as well as Players
    Given a Carcass that both an NPC and a Player seek
    Then both are able to draw from it
