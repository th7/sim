# Source: design/economy.md (damage/destroy a Structure), design/glossary.md (Structure)
Feature: Damage and destroy a Structure
  As a Player
  I want to damage and ultimately destroy Structures
  So that what is built in the shared world can also be torn down.

  Scenario: A Structure can be damaged
    Given a wooden palisade at full integrity
    When a Player damages it
    Then its remaining integrity is reduced

  Scenario: Enough damage destroys a Structure
    Given a wooden palisade
    When it is damaged enough to be destroyed
    Then the palisade no longer exists in the world
    And it no longer blocks Player movement
