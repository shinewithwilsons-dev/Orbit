//! Weather widget data from Open-Meteo (open-meteo.com): free, no API key.

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Day {
    pub date: String,
    pub min: f64,
    pub max: f64,
    pub desc: &'static str,
}

#[derive(Debug, Clone)]
pub struct Weather {
    pub place: String,
    pub temp: f64,
    pub wind: f64,
    pub desc: &'static str,
    pub days: Vec<Day>,
}

/// WMO weather code to words.
pub fn describe(code: i64) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 => "Rain",
        65 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71 | 73 | 75 | 77 => "Snow",
        80..=82 => "Showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm, hail",
        _ => "Unknown",
    }
}

pub fn fetch(city: &str) -> Result<Weather> {
    let geo: serde_json::Value = ureq::get(&format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
        crate::url_encode(city)
    ))
    .timeout(std::time::Duration::from_secs(10))
    .call()?
    .into_json()?;
    let r = &geo["results"][0];
    let lat = r["latitude"]
        .as_f64()
        .with_context(|| format!("couldn't find '{city}'"))?;
    let lon = r["longitude"].as_f64().context("no longitude")?;
    let place = format!(
        "{}{}",
        r["name"].as_str().unwrap_or(city),
        r["country_code"]
            .as_str()
            .map(|c| format!(", {c}"))
            .unwrap_or_default()
    );
    let f: serde_json::Value = ureq::get(&format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,weather_code,wind_speed_10m\
         &daily=temperature_2m_max,temperature_2m_min,weather_code&timezone=auto&forecast_days=4"
    ))
    .timeout(std::time::Duration::from_secs(10))
    .call()?
    .into_json()?;
    let c = &f["current"];
    let d = &f["daily"];
    let days = (0..d["time"].as_array().map(|a| a.len()).unwrap_or(0))
        .map(|i| Day {
            date: d["time"][i].as_str().unwrap_or("").to_string(),
            min: d["temperature_2m_min"][i].as_f64().unwrap_or(0.0),
            max: d["temperature_2m_max"][i].as_f64().unwrap_or(0.0),
            desc: describe(d["weather_code"][i].as_i64().unwrap_or(-1)),
        })
        .collect();
    Ok(Weather {
        place,
        temp: c["temperature_2m"].as_f64().unwrap_or(0.0),
        wind: c["wind_speed_10m"].as_f64().unwrap_or(0.0),
        desc: describe(c["weather_code"].as_i64().unwrap_or(-1)),
        days,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn codes() {
        assert_eq!(super::describe(0), "Clear");
        assert_eq!(super::describe(81), "Showers");
    }
}
