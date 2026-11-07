// From rustlings (https://github.com/rust-lang/rustlings), solutions/11_hashmaps/hashmaps3.rs.
// Copyright (c) 2016 Carol (Nichols || Goulding). MIT licence:
// https://github.com/rust-lang/rustlings/blob/main/LICENSE
// Changed: the tests module is dropped, and main prints what the tests check.

// A list of scores (one per line) of a soccer match is given. Each line is of
// the form "<team_1_name>,<team_2_name>,<team_1_goals>,<team_2_goals>"
// Example: "England,France,4,2" (England scored 4 goals, France 2).
//
// You have to build a table of scores containing the name of the team, the total
// number of goals the team scored, and the total number of goals the team
// conceded.

use std::collections::HashMap;

// A structure to store the goal details of a team.
#[derive(Default)]
struct TeamScores {
    goals_scored: u8,
    goals_conceded: u8,
}

fn build_score_table(results: &str) -> HashMap<&str, TeamScores> {
    // The name of the team is the key and its associated struct is the value.
    let mut scores = HashMap::<&str, TeamScores>::new();

    for line in results.lines() {
        let mut split_iterator = line.split(',');
        // NOTE: We use `unwrap` because we haven't dealt with error handling yet.
        let team_1_name = split_iterator.next().unwrap();
        let team_2_name = split_iterator.next().unwrap();
        let team_1_score: u8 = split_iterator.next().unwrap().parse().unwrap();
        let team_2_score: u8 = split_iterator.next().unwrap().parse().unwrap();

        // Insert the default with zeros if a team doesn't exist yet.
        let team_1 = scores.entry(team_1_name).or_default();
        // Update the values.
        team_1.goals_scored += team_1_score;
        team_1.goals_conceded += team_2_score;

        // Similarly for the second team.
        let team_2 = scores.entry(team_2_name).or_default();
        team_2.goals_scored += team_2_score;
        team_2.goals_conceded += team_1_score;
    }

    scores
}

const RESULTS: &str = "England,France,4,2
France,Italy,3,1
Poland,Spain,2,0
Germany,England,2,1
England,Spain,1,0";

fn main() {
    let scores = build_score_table(RESULTS);

    for team_name in ["England", "France", "Germany", "Italy", "Poland", "Spain"] {
        let team = scores.get(team_name).unwrap();
        println!(
            "{team_name}: scored {}, conceded {}",
            team.goals_scored, team.goals_conceded,
        );
    }
}
