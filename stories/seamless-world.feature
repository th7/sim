# Source: design/shared-world.md (seamless movement, one place shared), design/glossary.md (Chunk)
# Held pending designer: the one-authority / never-under-merge promise has no clean v1-observable
# (Players are invulnerable and do not interact with each other). See the upstream gap message.
Feature: Seamless world
  As a Player
  I want to move across the whole Overworld without seams
  So that the single shared world never reveals its internal partitioning.

  Scenario: Crossing an internal Chunk boundary is a non-event
    Given a Player moving through the Overworld toward a Chunk boundary
    When the Player crosses from one Chunk's area into the next
    Then movement remains continuous
    And the Player perceives no loading screen, stutter, or stall at the boundary

  Scenario: Reaching the edge of already-active space does not stall the Player
    Given a Player moving steadily in one direction across the Overworld
    When the Player reaches an area at the edge of what was already active
    Then the Player keeps moving without stalling on a not-yet-ready area
