//! Plausible fake readings for `hn simulate`.

use rand::Rng;

use crate::cli::SimProfile;

/// One round of values for `profile` at Unix time `t` (seconds). Smooth daily curves plus noise.
pub fn sample(profile: SimProfile, t: i64, rng: &mut impl Rng) -> Vec<(String, f64)> {
    // 0 at midnight UTC, peaks around 15:00
    let day = ((t % 86_400) as f64 / 86_400.0) * std::f64::consts::TAU;
    let daily = (day - std::f64::consts::TAU * 0.375).sin();
    let mut noise = |spread: f64| rng.random_range(-spread..=spread);
    let r = |x: f64| (x * 100.0).round() / 100.0;
    match profile {
        SimProfile::Weather => {
            let temp = 14.0 + 8.0 * daily + noise(0.4);
            vec![
                ("TEMPERATURE".into(), r(temp)),
                (
                    "HUMIDITY".into(),
                    r((70.0 - 18.0 * daily + noise(2.0)).clamp(20.0, 100.0)),
                ),
                (
                    "PRESSURE".into(),
                    r(1013.0 + 6.0 * (t as f64 / 172_800.0).sin() + noise(0.3)),
                ),
                (
                    "WIND_SPEED".into(),
                    r((3.0 + 2.0 * daily + noise(1.5)).max(0.0)),
                ),
            ]
        }
        SimProfile::Greenhouse => {
            let temp = 22.0 + 6.0 * daily + noise(0.3);
            vec![
                ("TEMPERATURE".into(), r(temp)),
                (
                    "HUMIDITY".into(),
                    r((65.0 - 10.0 * daily + noise(1.5)).clamp(30.0, 98.0)),
                ),
                (
                    "SOIL_MOISTURE".into(),
                    r((45.0 - 8.0 * (day / 2.0).sin().abs() + noise(1.0)).clamp(5.0, 95.0)),
                ),
                (
                    "CO2".into(),
                    r((650.0 - 200.0 * daily + noise(25.0)).max(400.0)),
                ),
                (
                    "LIGHT".into(),
                    r((daily.max(0.0) * 40_000.0 + noise(300.0)).max(0.0)),
                ),
            ]
        }
        SimProfile::Random => vec![
            ("TEMPERATURE".into(), r(rng.random_range(-10.0..=35.0))),
            ("HUMIDITY".into(), r(rng.random_range(10.0..=100.0))),
            ("BATTERY_VOLTAGE".into(), r(rng.random_range(3.3..=4.2))),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(values: &[(String, f64)], k: &str) -> f64 {
        values.iter().find(|(t, _)| t == k).unwrap().1
    }

    #[test]
    fn ranges_are_plausible_over_two_days() {
        let mut rng = rand::rng();
        for step in 0..(2 * 24 * 12) {
            let t = 1_730_000_000 + step * 300;
            let w = sample(SimProfile::Weather, t, &mut rng);
            assert!((4.0..=24.0).contains(&get(&w, "TEMPERATURE")));
            assert!((20.0..=100.0).contains(&get(&w, "HUMIDITY")));
            assert!((1000.0..=1025.0).contains(&get(&w, "PRESSURE")));
            assert!(get(&w, "WIND_SPEED") >= 0.0);
            let g = sample(SimProfile::Greenhouse, t, &mut rng);
            assert!((15.0..=29.0).contains(&get(&g, "TEMPERATURE")));
            assert!((5.0..=95.0).contains(&get(&g, "SOIL_MOISTURE")));
            assert!(get(&g, "CO2") >= 400.0);
            let r = sample(SimProfile::Random, t, &mut rng);
            assert!((3.3..=4.2).contains(&get(&r, "BATTERY_VOLTAGE")));
            for (ty, v) in w.iter().chain(&g).chain(&r) {
                assert!(hydronode_api::device::valid_type(ty));
                assert!(hydronode_api::device::format_value(*v).is_ok());
            }
        }
    }

    #[test]
    fn weather_is_warmer_in_the_afternoon() {
        let mut rng = rand::rng();
        let midnight = 1_731_801_600; // 2024-11-17 00:00 UTC
        let night = sample(SimProfile::Weather, midnight + 3 * 3600, &mut rng);
        let afternoon = sample(SimProfile::Weather, midnight + 15 * 3600, &mut rng);
        assert!(get(&afternoon, "TEMPERATURE") > get(&night, "TEMPERATURE") + 5.0);
    }
}
