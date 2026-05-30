# Source: design/living-world.md (animals: needs, not scripts),
#         design/glossary.md (NPC, Motivation, Need, Pressure, Goal, deer, wolf)
Feature: Animals pursue their needs
  As a Player
  I want animals to act on their own needs
  So that the wild world feels alive, not scripted.

  Scenario: A deer grazes when it is hungry and safe
    Given a deer with no threat nearby
    When the deer is hungry
    Then the deer moves to feed

  Scenario: A deer flees a threat instead of feeding
    Given a hungry deer
    When a wolf comes near
    Then the deer flees from the wolf rather than continuing to feed

  Scenario: A wolf hunts to satisfy its hunger
    Given a hungry wolf with a deer within reach
    When the wolf is hungry
    Then the wolf pursues the deer

  Scenario: A long-starving deer trades away safety to feed
    Given a deer that has gone hungry for a long time
    When a threat is present that it would normally flee
    Then the deer feeds despite the threat
