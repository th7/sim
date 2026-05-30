# Source: design/shared-world.md (the world persists and remembers), design/vision.md (pillar 2),
#         design/glossary.md (Datastore, Inventory, Structure, Resource node, Instance)
Feature: The world persists across restart
  As a Player
  I want what I did to the Overworld to survive a server restart
  So that persistence is a promise, not best-effort.

  Scenario Outline: Persisted facts survive a restart
    Given <fact> in the Overworld before a server restart
    When the server restarts
    Then <fact> is found exactly as it was

    Examples:
      | fact                                                |
      | a Player's last position and Inventory              |
      | a Structure's existence and its remaining integrity |
      | a Resource node's depletion and respawn timer       |

  Scenario: Instance state does not persist
    Given a Player inside an Instance with in-Instance state
    When the Player disconnects or the server restarts
    Then that in-Instance state is gone
