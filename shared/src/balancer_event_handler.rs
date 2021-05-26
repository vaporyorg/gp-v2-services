use crate::{
    current_block::BlockRetrieving,
    event_handling::{BlockNumber, EventHandler, EventIndex, EventStoring},
    impl_event_retrieving,
    maintenance::Maintaining,
    Web3,
};
use anyhow::{anyhow, Context, Result};
use contracts::{
    balancer_v2_vault::{
        self, event_data::PoolRegistered as ContractPoolRegistered, Event as ContractEvent,
    },
    BalancerV2Vault,
};
use ethcontract::common::DeploymentInformation;
use ethcontract::{dyns::DynWeb3, Event as EthContractEvent, EventMetadata, H160};
use std::{
    collections::{HashMap, HashSet},
    ops::RangeInclusive,
};
use tokio::sync::Mutex;

#[derive(Debug)]
pub enum BalancerEvent {
    PoolRegistered(PoolRegistered),
}

#[derive(Debug, Default)]
pub struct PoolRegistered {
    pub pool_id: [u8; 32],
    pub pool_address: H160,
    pub specialization: u8,
}

#[derive(Clone, Default, Eq, PartialEq, Hash)]
pub struct WeightedPool {
    pool_address: H160,
    tokens: Vec<H160>,
    pool_id: PoolId,
    // TODO - other stuff
}

pub type PoolId = [u8; 32];

#[derive(Default)]
pub struct BalancerPools {
    _pools_by_token: HashMap<H160, HashSet<PoolId>>,
    pools: HashMap<PoolId, WeightedPool>,
    // Block number of last update
    last_updated: u64,
}

impl BalancerPools {
    fn _update_last_block(mut self, value: u64) {
        self.last_updated = value;
    }

    // All insertions happen in one transaction.
    fn insert_events(&self, events: Vec<(EventIndex, BalancerEvent)>) -> Result<()> {
        for (index, event) in events {
            match event {
                BalancerEvent::PoolRegistered(event) => self.insert_pool(index, event),
            };
        }
        Ok(())
    }

    fn _delete_events(&self, _delete_from_block_number: u64) -> Result<()> {
        // TODO - delete from when asked.
        Ok(())
    }

    fn replace_events(
        &self,
        _delete_from_block_number: u64,
        events: Vec<(EventIndex, BalancerEvent)>,
    ) -> Result<()> {
        // self.delete_events(delete_from_block_number)?;
        self.insert_events(events)?;
        Ok(())
    }

    fn known_pool(&self, pool_id: PoolId) -> bool {
        self.pools.contains_key(&pool_id)
    }

    fn insert_pool(&self, index: EventIndex, registration: PoolRegistered) {
        if !self.known_pool(registration.pool_id) {
            let pool_tokens = vec![];
            let _weighted_pool = WeightedPool {
                pool_address: registration.pool_address,
                pool_id: registration.pool_id,
                tokens: pool_tokens,
            };
            // Need to figure out a way to update this.
            // self.pools
            //     .entry(registration.pool_id)
            //     .or_default()
            //     .insert(weighted_pool.clone());
            // for token in pool_tokens {
            //     self.pools_by_token
            //         .entry(token)
            //         .or_default()
            //         .insert(weighted_pool.pool_id);
            // }
            tracing::debug!(
                "Updated Balancer Pools with {:?} - {:?}",
                registration.pool_address,
                index
            );
        } else {
            tracing::debug!("Ignored known pool {:?}", registration.pool_address);
        }
    }

    fn contract_to_balancer_events(
        &self,
        contract_events: Vec<EthContractEvent<ContractEvent>>,
    ) -> Result<Vec<(EventIndex, BalancerEvent)>> {
        contract_events
            .into_iter()
            .filter_map(|EthContractEvent { data, meta }| {
                let meta = match meta {
                    Some(meta) => meta,
                    None => return Some(Err(anyhow!("event without metadata"))),
                };
                match data {
                    ContractEvent::PoolRegistered(event) => {
                        Some(convert_pool_registered(&event, &meta))
                    }
                    // ContractEvent::AuthorizerChanged(event) => {
                    //     tracing::debug!("Unhandled event {:?}", event);
                    //     None
                    // }
                    // ContractEvent::ExternalBalanceTransfer(event) => {
                    //     tracing::debug!("Unhandled event {:?}", event);
                    //     None
                    // }
                    // ContractEvent::FlashLoan(event) => {
                    //     tracing::debug!("Unhandled event {:?}", event);
                    //     None
                    // }
                    // ContractEvent::InternalBalanceChanged(event) => {
                    //     tracing::debug!("Unhandled event {:?}", event);
                    //     None
                    // }
                    // ContractEvent::PausedStateChanged(event) => {
                    //     tracing::debug!("Unhandled event {:?}", event);
                    //     None
                    // }
                    // ContractEvent::PoolBalanceChanged(event) => {
                    //     tracing::debug!("Unhandled event {:?}", event);
                    //     None
                    // }
                    // ContractEvent::PoolBalanceManaged(event) => {
                    //     tracing::debug!("Unhandled event {:?}", event);
                    //     None
                    // }
                    // ContractEvent::RelayerApprovalChanged(event) => {
                    //     tracing::debug!("Unhandled event {:?}", event);
                    //     None
                    // }
                    // ContractEvent::Swap(event) => {
                    //     tracing::debug!("Unhandled event {:?}", event);
                    //     None
                    // }
                    // ContractEvent::TokensDeregistered(event) => {
                    //     tracing::debug!("Tokens Deregistered {:?}", event);
                    //     None
                    // }
                    // ContractEvent::TokensRegistered(event) => {
                    //     tracing::debug!("Tokens Registered {:?}", event);
                    //     None
                    // }
                    _ => {
                        // TODO - Not processing other events at the moment.
                        None
                    }
                }
            })
            .collect::<Result<Vec<_>>>()
    }
}

pub struct BalancerPoolFetcher {
    pub web3: Web3,
}

impl BalancerPoolFetcher {
    async fn _get_pool_tokens(&self, _pool_address: H160) -> Vec<H160> {
        // let web3 = Web3::new(self.web3.transport().clone());
        // let pool_contract = BalancerPool::at(&web3, pool_address).await;
        // TODO - fetch tokens from pool
        // There are two different types of pools, hopefully they share a common interface.
        vec![]
    }
}

pub struct BalancerEventUpdater(
    Mutex<EventHandler<DynWeb3, BalancerV2VaultContract, BalancerPools>>,
);

impl BalancerEventUpdater {
    pub async fn new(contract: BalancerV2Vault, pools: BalancerPools) -> Self {
        let mut deployment_block = None;
        if let Some(deployment_info) = contract.deployment_information() {
            match deployment_info {
                DeploymentInformation::BlockNumber(block_number) => {
                    deployment_block = Some(block_number);
                }
                DeploymentInformation::TransactionHash(hash) => {
                    deployment_block = contract
                        .raw_instance()
                        .web3()
                        .block_number_from_tx_hash(hash)
                        .await;
                }
            }
        };
        Self(Mutex::new(EventHandler::new(
            contract.raw_instance().web3(),
            BalancerV2VaultContract(contract),
            pools,
            deployment_block,
        )))
    }
}

#[async_trait::async_trait]
impl EventStoring<ContractEvent> for BalancerPools {
    async fn replace_events(
        &self,
        events: Vec<EthContractEvent<ContractEvent>>,
        range: RangeInclusive<BlockNumber>,
    ) -> Result<()> {
        let balancer_events = self
            .contract_to_balancer_events(events)
            .context("failed to convert events")?;
        tracing::debug!(
            "replacing {} events from block number {}",
            balancer_events.len(),
            range.start().to_u64()
        );
        // Not sure if we even need this... since balancer team claims there will never be deregistered pools
        // However, it is still possible.
        self.replace_events(0, balancer_events)?;
        Ok(())
    }

    async fn append_events(&self, events: Vec<EthContractEvent<ContractEvent>>) -> Result<()> {
        let balancer_events = self
            .contract_to_balancer_events(events)
            .context("failed to convert events")?;
        self.insert_events(balancer_events)
    }

    async fn last_event_block(&self) -> Result<u64> {
        Ok(self.last_updated)
    }
}

impl_event_retrieving! {
    pub BalancerV2VaultContract for balancer_v2_vault
}

#[async_trait::async_trait]
impl Maintaining for BalancerEventUpdater {
    async fn run_maintenance(&self) -> Result<()> {
        self.0.run_maintenance().await
    }
}

fn convert_pool_registered(
    registration: &ContractPoolRegistered,
    meta: &EventMetadata,
) -> Result<(EventIndex, BalancerEvent)> {
    let event = PoolRegistered {
        pool_id: registration.pool_id,
        pool_address: registration.pool_address,
        specialization: registration.specialization,
    };
    Ok((EventIndex::from(meta), BalancerEvent::PoolRegistered(event)))
}
