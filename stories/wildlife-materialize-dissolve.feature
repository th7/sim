# Source: design/living-world.md (cheap where empty: materialize and dissolve),
#         design/glossary.md (Region, Disturbance, NPC)
Feature: Wildlife materializes near Players and dissolves when they leave
  As a Player
  I want wildlife to appear where I am and cost nothing where I am not
  So that the world is alive where it's seen and cheap where it's empty.

  Scenario: Wildlife appears as a Player approaches
    Given an area of the Overworld with no Player nearby
    When a Player approaches it
    Then wildlife appropriate to that Region appears

  Scenario: Wildlife dissolves when no Player remains nearby
    Given wildlife near a Player
    When the Player leaves and no Player remains nearby
    Then the wildlife is no longer simulated there

  Scenario: Animals have no persistent individual identity
    Given a Player who saw particular animals in an area
    When the Player leaves and later returns
    Then the Player finds wildlife consistent with that Region's level
    But not the same individual animals they saw before

  Scenario: Population and temperament reflect the Region's current level
    Given two Regions at different wildlife levels
    When a Player approaches each
    Then the count and temperament of the wildlife reflect each Region's level
