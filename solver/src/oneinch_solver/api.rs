//! 1Inch HTTP API client implementation.
//!
//! For more information on the HTTP API, consult:
//! <https://docs.1inch.io/api/quote-swap>
//! <https://api.1inch.exchange/swagger/ethereum/>

use anyhow::{ensure, Result};
use ethcontract::{H160, U256};
use reqwest::{Client, IntoUrl, Url};
use serde::{
    de::{Deserializer, Error as _},
    Deserialize,
};
use shared::http::default_http_client;
use std::{
    borrow::Cow,
    fmt::{self, Display, Formatter},
};

/// A slippage amount.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Slippage(f64);

impl Slippage {
    /// Creates a slippage amount from the specified percentage.
    pub fn percentage(amount: f64) -> Result<Self> {
        // 1Inch API only accepts a slippage from 0 to 50.
        ensure!(
            (0. ..=50.).contains(&amount),
            "slippage outside of [0%, 50%] range"
        );
        Ok(Slippage(amount))
    }

    /// Creates a slippage amount from the specified basis points.
    pub fn basis_points(bps: u16) -> Result<Self> {
        let percent = (bps as f64) / 100.;
        Slippage::percentage(percent)
    }
}

impl Display for Slippage {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Parts to split a swap.
///
/// This type is generic on the maximum number of splits allowed.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Amount<const MIN: usize, const MAX: usize>(usize);

impl<const MIN: usize, const MAX: usize> Amount<MIN, MAX> {
    /// Creates a parts amount from the specified count.
    pub fn new(amount: usize) -> Result<Self> {
        // 1Inch API only accepts a slippage from 0 to 50.
        ensure!(
            (MIN..=MAX).contains(&amount),
            "parts outside of [{}, {}] range",
            MIN,
            MAX,
        );
        Ok(Amount(amount))
    }
}

impl<const MIN: usize, const MAX: usize> Display for Amount<MIN, MAX> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A 1Inch API quote query parameters.
///
/// These parameters are currently incomplete, and missing parameters can be
/// added incrementally as needed.
#[derive(Clone, Debug)]
pub struct SwapQuery {
    /// Contract address of a token to sell.
    pub from_token_address: H160,
    /// Contract address of a token to buy.
    pub to_token_address: H160,
    /// Amount of a token to sell, set in atoms.
    pub amount: U256,
    /// Address of a seller.
    ///
    /// Make sure that this address has approved to spend `from_token_address`
    /// in needed amount.
    pub from_address: H160,
    /// Limit of price slippage you are willing to accept.
    pub slippage: Slippage,
    /// Flag to disable checks of the required quantities.
    pub disable_estimate: Option<bool>,
    /// Maximum number of token-connectors to be used in a transaction.
    pub complexity_level: Option<Amount<0, 3>>,
    /// Maximum amount of gas for a swap.
    pub gas_limit: Option<u64>,
    /// Limit maximum number of main route parts.
    pub main_route_parts: Option<Amount<1, 50>>,
    /// Limit maximum number of parts each main route part can be split into.
    pub parts: Option<Amount<1, 100>>,
}

impl SwapQuery {
    /// Encodes the swap query as
    fn into_url(self, base_url: &Url) -> Url {
        // The `Display` implementation for `H160` unfortunately does not print
        // the full address and instead uses ellipsis (e.g. "0xeeee???eeee"). This
        // helper just works around that.
        fn addr2str(addr: H160) -> String {
            format!("{:?}", addr)
        }

        let mut url = base_url
            .join("v3.0/1/swap")
            .expect("unexpectedly invalid URL segment");
        url.query_pairs_mut()
            .append_pair("fromTokenAddress", &addr2str(self.from_token_address))
            .append_pair("toTokenAddress", &addr2str(self.to_token_address))
            .append_pair("amount", &self.amount.to_string())
            .append_pair("fromAddress", &addr2str(self.from_address))
            .append_pair("slippage", &self.slippage.to_string());

        if let Some(disable_estimate) = self.disable_estimate {
            url.query_pairs_mut()
                .append_pair("disableEstimate", &disable_estimate.to_string());
        }
        if let Some(complexity_level) = self.complexity_level {
            url.query_pairs_mut()
                .append_pair("complexityLevel", &complexity_level.to_string());
        }
        if let Some(gas_limit) = self.gas_limit {
            url.query_pairs_mut()
                .append_pair("gasLimit", &gas_limit.to_string());
        }
        if let Some(main_route_parts) = self.main_route_parts {
            url.query_pairs_mut()
                .append_pair("mainRouteParts", &main_route_parts.to_string());
        }
        if let Some(parts) = self.parts {
            url.query_pairs_mut()
                .append_pair("parts", &parts.to_string());
        }

        url
    }
}

/// A 1Inch API swap response.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Swap {
    pub from_token: Token,
    pub to_token: Token,
    #[serde(deserialize_with = "deserialize_decimal_u256")]
    pub from_token_amount: U256,
    #[serde(deserialize_with = "deserialize_decimal_u256")]
    pub to_token_amount: U256,
    pub protocols: Vec<Vec<Vec<Protocol>>>,
    pub tx: Transaction,
}

/// Metadata associated with a token.
///
/// The response data is currently incomplete, and missing fields can be added
/// incrementally as needed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Token {
    pub address: H160,
}

/// Metadata associated with a protocol used for part of a 1Inch swap.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Protocol {
    pub name: String,
    pub part: f64,
    pub from_token_address: H160,
    pub to_token_address: H160,
}

/// Swap transaction generated by the 1Inch API.
#[derive(Clone, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub from: H160,
    pub to: H160,
    #[serde(deserialize_with = "deserialize_prefixed_hex")]
    pub data: Vec<u8>,
    #[serde(deserialize_with = "deserialize_decimal_u256")]
    pub value: U256,
    #[serde(deserialize_with = "deserialize_decimal_u256")]
    pub gas_price: U256,
    pub gas: u64,
}

impl std::fmt::Debug for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transaction")
            .field("from", &self.from)
            .field("to", &self.to)
            .field("data", &format_args!("0x{}", hex::encode(&self.data)))
            .field("value", &self.value)
            .field("gas_price", &self.gas_price)
            .field("gas", &self.gas)
            .finish()
    }
}

fn deserialize_decimal_u256<'de, D>(deserializer: D) -> Result<U256, D::Error>
where
    D: Deserializer<'de>,
{
    let decimal_str = Cow::<str>::deserialize(deserializer)?;
    U256::from_dec_str(&*decimal_str).map_err(D::Error::custom)
}

fn deserialize_prefixed_hex<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let prefixed_hex_str = Cow::<str>::deserialize(deserializer)?;
    let hex_str = prefixed_hex_str
        .strip_prefix("0x")
        .ok_or_else(|| D::Error::custom("hex missing '0x' prefix"))?;
    hex::decode(hex_str).map_err(D::Error::custom)
}

/// Approve spender response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Spender {
    pub address: H160,
}

/// 1Inch API Client implementation.
#[derive(Debug)]
pub struct OneInchClient {
    client: Client,
    base_url: Url,
}

impl OneInchClient {
    /// Create a new 1Inch HTTP API client with the specified base URL.
    pub fn new(base_url: impl IntoUrl) -> Result<Self> {
        Ok(Self {
            client: default_http_client()?,
            base_url: base_url.into_url()?,
        })
    }

    /// Retrieves a swap for the specified parameters from the 1Inch API.
    pub async fn get_swap(&self, query: SwapQuery) -> Result<Swap> {
        Ok(self
            .client
            .get(query.into_url(&self.base_url))
            .send()
            .await?
            .json()
            .await?)
    }

    /// Retrieves the address of the spender to use for token approvals.
    pub async fn get_spender(&self) -> Result<Spender> {
        let url = self
            .base_url
            .join("v3.0/1/approve/spender")
            .expect("unexpectedly invalid URL");
        Ok(self.client.get(url).send().await?.json().await?)
    }
}

impl Default for OneInchClient {
    fn default() -> Self {
        Self::new("https://api.1inch.exchange/").expect("unexpected error parsing URL")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slippage_from_basis_points() {
        assert_eq!(
            Slippage::basis_points(50).unwrap(),
            Slippage::percentage(0.5).unwrap(),
        )
    }

    #[test]
    fn slippage_out_of_range() {
        assert!(Slippage::percentage(-1.).is_err());
        assert!(Slippage::percentage(1337.).is_err());
    }

    #[test]
    fn amounts_valid_range() {
        assert!(Amount::<42, 1337>::new(41).is_err());
        assert!(Amount::<42, 1337>::new(42).is_ok());
        assert!(Amount::<42, 1337>::new(1337).is_ok());
        assert!(Amount::<42, 1337>::new(1338).is_err());
    }

    #[test]
    fn swap_query_serialization() {
        let base_url = Url::parse("https://api.1inch.exchange/").unwrap();
        let url = SwapQuery {
            from_token_address: shared::addr!("EeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"),
            to_token_address: shared::addr!("111111111117dc0aa78b770fa6a738034120c302"),
            amount: 1_000_000_000_000_000_000u128.into(),
            from_address: shared::addr!("00000000219ab540356cBB839Cbe05303d7705Fa"),
            slippage: Slippage::basis_points(50).unwrap(),
            disable_estimate: None,
            complexity_level: None,
            gas_limit: None,
            main_route_parts: None,
            parts: None,
        }
        .into_url(&base_url);

        assert_eq!(
            url.as_str(),
            "https://api.1inch.exchange/v3.0/1/swap\
                ?fromTokenAddress=0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee\
                &toTokenAddress=0x111111111117dc0aa78b770fa6a738034120c302\
                &amount=1000000000000000000\
                &fromAddress=0x00000000219ab540356cbb839cbe05303d7705fa\
                &slippage=0.5",
        );
    }

    #[test]
    fn swap_query_serialization_options_parameters() {
        let base_url = Url::parse("https://api.1inch.exchange/").unwrap();
        let url = SwapQuery {
            from_token_address: shared::addr!("EeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"),
            to_token_address: shared::addr!("111111111117dc0aa78b770fa6a738034120c302"),
            amount: 1_000_000_000_000_000_000u128.into(),
            from_address: shared::addr!("00000000219ab540356cBB839Cbe05303d7705Fa"),
            slippage: Slippage::basis_points(50).unwrap(),
            disable_estimate: Some(true),
            complexity_level: Some(Amount::new(1).unwrap()),
            gas_limit: Some(133700),
            main_route_parts: Some(Amount::new(28).unwrap()),
            parts: Some(Amount::new(42).unwrap()),
        }
        .into_url(&base_url);

        assert_eq!(
            url.as_str(),
            "https://api.1inch.exchange/v3.0/1/swap\
                ?fromTokenAddress=0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee\
                &toTokenAddress=0x111111111117dc0aa78b770fa6a738034120c302\
                &amount=1000000000000000000\
                &fromAddress=0x00000000219ab540356cbb839cbe05303d7705fa\
                &slippage=0.5\
                &disableEstimate=true\
                &complexityLevel=1\
                &gasLimit=133700\
                &mainRouteParts=28\
                &parts=42",
        );
    }

    #[test]
    fn deserialize_swap_response() {
        let swap = serde_json::from_str::<Swap>(
            r#"{
              "fromToken": {
                "symbol": "ETH",
                "name": "Ethereum",
                "decimals": 18,
                "address": "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                "logoURI": "https://tokens.1inch.exchange/0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee.png"
              },
              "toToken": {
                "symbol": "1INCH",
                "name": "1INCH Token",
                "decimals": 18,
                "address": "0x111111111117dc0aa78b770fa6a738034120c302",
                "logoURI": "https://tokens.1inch.exchange/0x111111111117dc0aa78b770fa6a738034120c302.png"
              },
              "toTokenAmount": "501739725821378713485",
              "fromTokenAmount": "1000000000000000000",
              "protocols": [
                [
                  [
                    {
                      "name": "WETH",
                      "part": 100,
                      "fromTokenAddress": "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                      "toTokenAddress": "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"
                    }
                  ],
                  [
                    {
                      "name": "UNISWAP_V2",
                      "part": 100,
                      "fromTokenAddress": "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
                      "toTokenAddress": "0x111111111117dc0aa78b770fa6a738034120c302"
                    }
                  ]
                ]
              ],
              "tx": {
                "from": "0x00000000219ab540356cBB839Cbe05303d7705Fa",
                "to": "0x11111112542d85b3ef69ae05771c2dccff4faa26",
                "data": "0x2e95b6c800000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000de0b6b3a764000000000000000000000000000000000000000000000000001b1038e63128bd548d0000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000000000000180000000000000003b6d034026aad2da94c59524ac0d93f6d6cbf9071d7086f2",
                "value": "1000000000000000000",
                "gasPrice": "154110000000",
                "gas": 143297
              }
            }"#,
        )
        .unwrap();

        assert_eq!(
            swap,
            Swap {
                from_token: Token {
                    address: shared::addr!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
                },
                to_token: Token {
                    address: shared::addr!("111111111117dc0aa78b770fa6a738034120c302"),
                },
                from_token_amount: 1_000_000_000_000_000_000u128.into(),
                to_token_amount: 501_739_725_821_378_713_485u128.into(),
                protocols: vec![vec![
                    vec![Protocol {
                        name: "WETH".to_owned(),
                        part: 100.,
                        from_token_address: shared::addr!(
                            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                        ),
                        to_token_address: shared::addr!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
                    }],
                    vec![Protocol {
                        name: "UNISWAP_V2".to_owned(),
                        part: 100.,
                        from_token_address: shared::addr!(
                            "c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"
                        ),
                        to_token_address: shared::addr!("111111111117dc0aa78b770fa6a738034120c302"),
                    }],
                ]],
                tx: Transaction {
                    from: shared::addr!("00000000219ab540356cBB839Cbe05303d7705Fa"),
                    to: shared::addr!("11111112542d85b3ef69ae05771c2dccff4faa26"),
                    data: hex::decode(
                        "2e95b6c8\
                         0000000000000000000000000000000000000000000000000000000000000000\
                         0000000000000000000000000000000000000000000000000de0b6b3a7640000\
                         00000000000000000000000000000000000000000000001b1038e63128bd548d\
                         0000000000000000000000000000000000000000000000000000000000000080\
                         0000000000000000000000000000000000000000000000000000000000000001\
                         80000000000000003b6d034026aad2da94c59524ac0d93f6d6cbf9071d7086f2"
                    )
                    .unwrap(),
                    value: 1_000_000_000_000_000_000u128.into(),
                    gas_price: 154_110_000_000u128.into(),
                    gas: 143297,
                },
            }
        );
    }

    #[test]
    fn deserialize_spender_response() {
        let spender = serde_json::from_str::<Spender>(
            r#"{
              "address": "0x11111112542d85b3ef69ae05771c2dccff4faa26"
            }"#,
        )
        .unwrap();

        assert_eq!(
            spender,
            Spender {
                address: shared::addr!("11111112542d85b3ef69ae05771c2dccff4faa26"),
            }
        )
    }

    #[tokio::test]
    #[ignore]
    async fn oneinch_swap() {
        let swap = OneInchClient::default()
            .get_swap(SwapQuery {
                from_token_address: shared::addr!("EeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"),
                to_token_address: shared::addr!("111111111117dc0aa78b770fa6a738034120c302"),
                amount: 1_000_000_000_000_000_000u128.into(),
                from_address: shared::addr!("00000000219ab540356cBB839Cbe05303d7705Fa"),
                slippage: Slippage::basis_points(50).unwrap(),
                disable_estimate: None,
                complexity_level: None,
                gas_limit: None,
                main_route_parts: None,
                parts: None,
            })
            .await
            .unwrap();
        println!("{:#?}", swap);
    }

    #[tokio::test]
    #[ignore]
    async fn oneinch_swap_without_amount_checks_and_splitting() {
        let swap = OneInchClient::default()
            .get_swap(SwapQuery {
                from_token_address: shared::addr!("EeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"),
                to_token_address: shared::addr!("a3BeD4E1c75D00fa6f4E5E6922DB7261B5E9AcD2"),
                amount: 100_000_000_000_000_000_000u128.into(),
                from_address: shared::addr!("4e608b7da83f8e9213f554bdaa77c72e125529d0"),
                slippage: Slippage::basis_points(50).unwrap(),
                disable_estimate: Some(true),
                complexity_level: Some(Amount::new(2).unwrap()),
                gas_limit: Some(750_000),
                main_route_parts: Some(Amount::new(3).unwrap()),
                parts: Some(Amount::new(3).unwrap()),
            })
            .await
            .unwrap();
        println!("{:#?}", swap);
    }

    #[tokio::test]
    #[ignore]
    async fn oneinch_spender_address() {
        let spender = OneInchClient::default().get_spender().await.unwrap();
        println!("{:#?}", spender);
    }
}
