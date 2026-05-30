# Source: design/vision.md (shape of a session), design/shared-world.md (persists & remembers),
#         design/glossary.md (Player, Inventory)
Feature: Connect and resume
  As a Player
  I want to connect under my username and pick up exactly where I left off
  So that the shared world remembers my presence between sessions.

  Scenario: A new Player connects under a username
    Given no Player has ever connected as "ada"
    When a Player connects under the username "ada"
    Then a single in-world Player entity exists for "ada"
    And that entity is the only one controlled by "ada"

  Scenario: A returning Player resumes where they logged off
    Given a Player "ada" logged off at a known position with a known Inventory
    When "ada" connects again under the same username
    Then "ada" resumes at the position they logged off from
    And "ada"'s Inventory is exactly what it was at logoff
