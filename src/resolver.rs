use crate::{AssetClass, Instrument, SessionCalendar};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub instrument: Instrument,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Resolution {
    Found { instrument: Instrument },
    AmbiguousAsset { candidates: Vec<Instrument> },
    UnsupportedAsset { query: String },
}

#[derive(Debug, Clone)]
pub struct AssetResolver {
    entries: Vec<CatalogEntry>,
}

impl Default for AssetResolver {
    fn default() -> Self {
        Self::new(default_catalog())
    }
}

impl AssetResolver {
    pub fn new(entries: Vec<CatalogEntry>) -> Self {
        Self { entries }
    }

    pub fn resolve(&self, query: &str) -> Resolution {
        let normalized = normalize(query);
        let mut candidates: Vec<Instrument> = self
            .entries
            .iter()
            .filter(|entry| {
                normalize(&entry.instrument.symbol) == normalized
                    || entry
                        .aliases
                        .iter()
                        .any(|alias| normalize(alias) == normalized)
            })
            .map(|entry| entry.instrument.clone())
            .collect();
        candidates.sort_by(|a, b| a.instrument_id.cmp(&b.instrument_id));
        candidates.dedup_by(|a, b| a.instrument_id == b.instrument_id);
        match candidates.len() {
            0 => Resolution::UnsupportedAsset {
                query: query.to_owned(),
            },
            1 => Resolution::Found {
                instrument: candidates.remove(0),
            },
            _ => Resolution::AmbiguousAsset { candidates },
        }
    }
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect()
}

fn instrument(
    id: &str,
    asset_class: AssetClass,
    symbol: &str,
    base: Option<&str>,
    quote: Option<&str>,
    venue: &str,
    calendar: SessionCalendar,
) -> Instrument {
    Instrument {
        instrument_id: id.to_owned(),
        asset_class,
        symbol: symbol.to_owned(),
        base: base.map(str::to_owned),
        quote: quote.map(str::to_owned),
        venue: venue.to_owned(),
        timezone: "UTC".to_owned(),
        session_calendar: calendar,
        price_precision: None,
        quantity_precision: None,
        live_data_capable: true,
        historical_data_capable: true,
    }
}

pub fn default_catalog() -> Vec<CatalogEntry> {
    vec![
        CatalogEntry {
            instrument: instrument(
                "crypto:binance:BTCUSDT",
                AssetClass::Crypto,
                "BTCUSDT",
                Some("BTC"),
                Some("USDT"),
                "binance",
                SessionCalendar::ContinuousUtc,
            ),
            aliases: vec!["BTC".into(), "BTCUSDT".into()],
        },
        CatalogEntry {
            instrument: instrument(
                "crypto:binance:XRPUSDT",
                AssetClass::Crypto,
                "XRPUSDT",
                Some("XRP"),
                Some("USDT"),
                "binance",
                SessionCalendar::ContinuousUtc,
            ),
            aliases: vec!["XRP".into(), "XRPUSDT".into()],
        },
        CatalogEntry {
            instrument: instrument(
                "stock:massive:AAPL",
                AssetClass::Stock,
                "AAPL",
                Some("AAPL"),
                Some("USD"),
                "massive",
                SessionCalendar::ExchangeSession,
            ),
            aliases: vec!["AAPL".into()],
        },
        CatalogEntry {
            instrument: instrument(
                "stock:massive:MSFT",
                AssetClass::Stock,
                "MSFT",
                Some("MSFT"),
                Some("USD"),
                "massive",
                SessionCalendar::ExchangeSession,
            ),
            aliases: vec!["MSFT".into()],
        },
        CatalogEntry {
            instrument: instrument(
                "index:massive:SPX",
                AssetClass::Index,
                "SPX",
                None,
                Some("USD"),
                "massive",
                SessionCalendar::ExchangeSession,
            ),
            aliases: vec!["SPX".into(), "SP500".into(), "S&P500".into()],
        },
        CatalogEntry {
            instrument: instrument(
                "index:massive:NDX",
                AssetClass::Index,
                "NDX",
                None,
                Some("USD"),
                "massive",
                SessionCalendar::ExchangeSession,
            ),
            aliases: vec!["NDX".into(), "NASDAQ".into()],
        },
        CatalogEntry {
            instrument: instrument(
                "forex:massive:EURUSD",
                AssetClass::Forex,
                "EURUSD",
                Some("EUR"),
                Some("USD"),
                "massive",
                SessionCalendar::ExchangeSession,
            ),
            aliases: vec!["EURUSD".into(), "EUR/USD".into()],
        },
        CatalogEntry {
            instrument: instrument(
                "futures:massive:GC",
                AssetClass::Futures,
                "GC",
                Some("GOLD"),
                Some("USD"),
                "massive",
                SessionCalendar::ExchangeSession,
            ),
            aliases: vec!["GC".into(), "GOLD".into()],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_known_alias() {
        let result = AssetResolver::default().resolve("btc");
        assert!(
            matches!(result, Resolution::Found { instrument } if instrument.symbol == "BTCUSDT")
        );
    }

    #[test]
    fn never_guesses_ambiguity() {
        let mut entries = default_catalog();
        entries[1].aliases.push("BTC".into());
        assert!(matches!(
            AssetResolver::new(entries).resolve("BTC"),
            Resolution::AmbiguousAsset { .. }
        ));
    }
}
