# Source: design/shared-world.md (Instances — private content), design/vision.md (shape of a session),
#         design/glossary.md (Instance, Portal, Party, Instance entry/exit)
# Held pending designer: how a multi-member Party forms and whether members enter together on one
# Portal overlap is not yet designed. These scenarios are written at the single-Player-observable
# level that generalizes to "the last Player remaining". See the upstream gap message.
Feature: Instances — private content off the shared world
  As a Player
  I want to step through a Portal into a private Instance and return where I left
  So that dungeon content is mine and my Party's, off the shared world.

  Scenario: Entering an Instance through a Portal
    Given a Player in the Overworld overlapping an into-instance Portal
    When the Player enters the Instance
    Then the Player is re-homed into the Instance
    And what the Player sees switches to the Instance's space

  Scenario: An Instance carries no shared-world fixtures
    Given a Player inside an Instance
    Then the Instance contains no Resource nodes and no Structures

  Scenario: Exiting returns the Player to where they entered
    Given a Player who entered an Instance from a place in the Overworld
    When the Player overlaps the return Portal
    Then the Player returns to the Overworld beside the Portal they entered from, at a small offset

  Scenario: Disconnecting inside an Instance returns the Player beside the entry Portal
    Given a Player inside an Instance
    When the Player disconnects and later reconnects
    Then the Player is in the Overworld next to the entry Portal
    And the Player is not looped back into the Instance

  Scenario: An Instance is destroyed when no one remains in it
    Given an Instance with Players inside
    When the last Player leaves or disconnects
    Then the Instance is destroyed
    And its in-Instance state is gone
