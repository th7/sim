# Source: design/vision.md (continuous, no grid steps), design/shared-world.md (seamless),
#         design/glossary.md (Footprint, Resource node, Structure, Player)
Feature: Continuous movement and collision
  As a Player
  I want to move freely and be blocked only by solid things in the world
  So that the world feels continuous and physical.

  Scenario: Movement is free and continuous
    Given a Player standing in open Overworld space
    When the Player moves
    Then the Player's position changes continuously, not in discrete steps

  Scenario: The world blocks the Player at a Footprint
    Given a Resource node with a Footprint ahead of the Player
    When the Player moves so their body would overlap the Footprint
    Then the Player is stopped at the Footprint and does not pass through it

  Scenario: Collision is one-way — the Player blocks nothing
    Given two Players standing in the same open space
    When one Player moves through the space the other occupies
    Then neither Player blocks the other

  Scenario: A depleted Resource node still blocks
    Given a Resource node that has been harvested to depletion
    When the Player moves so their body would overlap its Footprint
    Then the Player is still blocked
