# Source: design/living-world.md (the world has history: regions, baseline, disturbance),
#         design/glossary.md (Region, Habitat, Baseline, Disturbance)
# Note: durable (cross-restart) Disturbance is named but NOT yet designed — out of scope here.
Feature: Regions deplete when overhunted and heal over time
  As a Player
  I want overhunting to leave a mark that heals
  So that the world carries a legible history of how it has been used.

  Scenario: Overhunting depletes a Region
    Given a Region at its baseline wildlife level
    When Players overhunt it
    Then the Region's wildlife level falls below baseline

  Scenario: A depleted Region spawns fewer and more aggressive animals
    Given a depleted, overhunted Region
    When a Player approaches it
    Then fewer animals appear than at baseline
    And the animals that appear are hungrier and more aggressive

  Scenario: A Region heals when left alone
    Given a depleted Region
    When it is left unhunted for a long time
    Then its wildlife level recovers back toward baseline
