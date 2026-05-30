# Source: design/shared-world.md (persistence is not best-effort), design/glossary.md (Backpressure, Datastore, Island)
# Derived from the persistence-is-a-promise commitment rather than an explicit story-ready bullet;
# flagged to the designer for v1-scope confirmation (see the upstream gap message).
Feature: Overload freezes Players rather than losing state
  As a Player
  I want the world to pause rather than lose what I did when it is overloaded
  So that persistence stays a promise even under load.

  Scenario: Under sustained overload, affected Players freeze instead of losing state
    Given the world's persistence cannot keep up with demand
    When this affects a group of Players who share one authority
    Then those Players' inputs stall — they freeze together
    And no Player's state is dropped or corrupted
    And the system does not crash

  Scenario: Play resumes when the overload clears
    Given Players frozen because persistence could not keep up
    When the persistence subsystem recovers
    Then the frozen Players resume play
    And the actions they took before the freeze are intact
