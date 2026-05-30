# Source: design/economy.md (build), design/glossary.md (Structure, wooden palisade, wood, Inventory, Footprint)
Feature: Build a Structure
  As a Player
  I want to spend gathered materials to place a wooden palisade
  So that my effort leaves a persistent mark on the shared world.

  Background:
    Given the only buildable Structure is a wooden palisade costing 5 wood

  Scenario: Building a wooden palisade spends its cost
    Given a Player whose Inventory holds at least 5 wood
    When the Player builds a wooden palisade at a buildable location
    Then a wooden palisade exists at that location
    And 5 wood is removed from the Player's Inventory
    And the wooden palisade is owned by the Player who placed it

  Scenario: A built Structure blocks Player movement
    Given a wooden palisade has been placed
    When a Player moves so their body would overlap its Footprint
    Then the Player is blocked by the palisade

  Scenario: Building requires enough wood
    Given a Player whose Inventory holds fewer than 5 wood
    When the Player attempts to build a wooden palisade
    Then no wooden palisade is created
    And no wood is removed from the Player's Inventory
