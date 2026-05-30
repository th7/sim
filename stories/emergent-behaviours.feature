# Source: design/living-world.md (emergent behaviour is the goal, not extra rules),
#         design/glossary.md (deer, wolf, NPC)
Feature: Emergent group behaviours
  As a Player
  I want group behaviour to emerge from animals following their needs near each other
  So that the wild world produces believable collective life.

  Scenario: Deer herd together
    Given several deer in the same area
    Then the deer tend to stay grouped and move together

  Scenario: A startled herd stampedes
    Given a herd of deer
    When the herd is startled by a threat
    Then the deer flee together as a stampede

  Scenario: Wolves pack-hunt
    Given several wolves with prey nearby
    When the wolves hunt
    Then they coordinate to bring down prey together

  Scenario: Animals are bolder at night
    Given the same animal by day and by night
    Then the animal ventures more boldly at night than by day

  Scenario: A wounded animal is warier
    Given an animal that has been wounded
    Then the animal behaves more cautiously than when unhurt
