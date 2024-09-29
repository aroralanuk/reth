use crate::{
    BuildArguments, BuildOutcome, PayloadBuilder, PayloadBuilderError,
    PayloadConfig, PayloadBuilderAttributes
};

use alloy_primitives::{Address, B256};
use reth_payload_builder::PayloadId;
use reth_payload_primitives::BuiltPayload;
use reth_primitives::{SealedBlock, Withdrawals, U256};

use std::fmt;
use std::error::Error;

/// hand rolled Either enum to handle two builder types
#[derive(Debug, Clone)]
pub enum Either<L, R> {
    /// left variant
    Left(L),
    /// right variant
    Right(R),
}

impl<L, R> fmt::Display for Either<L, R>
where
    L: fmt::Display,
    R: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Either::Left(l) => write!(f, "Left: {}", l),
            Either::Right(r) => write!(f, "Right: {}", r),
        }
    }
}

impl<L, R> Error for Either<L, R>
where
    L: Error + 'static,
    R: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Either::Left(l) => Some(l),
            Either::Right(r) => Some(r),
        }
    }
}

impl<L, R> PayloadBuilderAttributes for Either<L, R>
   where
       L: PayloadBuilderAttributes,
       R: PayloadBuilderAttributes,
       L::Error: Error + 'static, 
       R::Error: Error + 'static,
   {
       type RpcPayloadAttributes = Either<L::RpcPayloadAttributes, R::RpcPayloadAttributes>;
       type Error = Either<L::Error, R::Error>;
   
       fn try_new(
           parent: B256,
           rpc_payload_attributes: Self::RpcPayloadAttributes,
       ) -> Result<Self, Self::Error> {
           match rpc_payload_attributes {
               Either::Left(attr) => L::try_new(parent, attr).map(Either::Left).map_err(Either::Left),
               Either::Right(attr) => R::try_new(parent, attr).map(Either::Right).map_err(Either::Right),
           }
       }
   
       fn payload_id(&self) -> PayloadId {
           match self {
               Either::Left(l) => l.payload_id(),
               Either::Right(r) => r.payload_id(),
           }
       }

       fn parent(&self) -> B256 {
           match self {
               Either::Left(l) => l.parent(),
               Either::Right(r) => r.parent(),
           }
       }
   
       fn timestamp(&self) -> u64 {
           match self {
               Either::Left(l) => l.timestamp(),
               Either::Right(r) => r.timestamp(),
           }
       }

       fn parent_beacon_block_root(&self) -> Option<B256> {
            match self {
                Either::Left(l) => l.parent_beacon_block_root(),
                Either::Right(r) => r.parent_beacon_block_root(),   
            }
       }

       fn suggested_fee_recipient(&self) -> Address {
            match self {
                Either::Left(l) => l.suggested_fee_recipient(),
                Either::Right(r) => r.suggested_fee_recipient(),
            }
       }

       fn prev_randao(&self) -> B256 {
            match self {
                Either::Left(l) => l.prev_randao(),
                Either::Right(r) => r.prev_randao(),
            }
       }

       fn withdrawals(&self) -> &Withdrawals {
            match self {
                Either::Left(l) => l.withdrawals(),
                Either::Right(r) => r.withdrawals(),
            }
       }
   }

/// this structure enables the chaining of multiple `PayloadBuilder` implementations,
/// creating a hierarchical fallback system. It's designed to be nestable, allowing
/// for complex builder arrangements like `Stack<Stack<A, B>, C>` with different
#[derive(Debug)]
pub struct PayloadBuilderStack<L, R> {
    left: L,
    right: R,
}

impl<L, R> PayloadBuilderStack<L, R> {
    /// Creates a new `PayloadBuilderStack` with the given left and right builders.
    pub const fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<L, R> Clone for PayloadBuilderStack<L, R>
where
    L: Clone,
    R: Clone,
{
    fn clone(&self) -> Self {
        Self::new(self.left.clone(), self.right.clone())
    }
}

impl<L, R> BuiltPayload for Either<L, R>
where
    L: BuiltPayload,
    R: BuiltPayload,
{
    fn block(&self) -> &SealedBlock {
        match self {
            Either::Left(l) => l.block(),
            Either::Right(r) => r.block(),
        }
    }

    fn fees(&self) -> U256 {
        match self {
            Either::Left(l) => l.fees(),
            Either::Right(r) => r.fees(),
        }
    }
}

impl<B> BuildOutcome<B> {
    fn map_payload<F, B2>(self, f: F) -> BuildOutcome<B2>
    where
        F: FnOnce(B) -> B2,
    {
        match self {
            BuildOutcome::Better { payload, cached_reads } => BuildOutcome::Better {
                payload: f(payload),
                cached_reads,
            },
            BuildOutcome::Aborted { fees, cached_reads } => BuildOutcome::Aborted { fees, cached_reads },
            BuildOutcome::Cancelled => BuildOutcome::Cancelled,
        }
    }
}

impl<L, R, Pool, Client> PayloadBuilder<Pool, Client> for PayloadBuilderStack<L, R>
where
    L: PayloadBuilder<Pool, Client> + Unpin + 'static,
    R: PayloadBuilder<Pool, Client> + Unpin + 'static,
    Client: Clone,
    Pool: Clone,
    L::Attributes: Unpin + Clone,
    R::Attributes: Unpin + Clone,
    L::BuiltPayload: Unpin + Clone,
    R::BuiltPayload: Unpin + Clone,
    <<L as PayloadBuilder<Pool, Client>>::Attributes as PayloadBuilderAttributes>::Error: 'static,
    <<R as PayloadBuilder<Pool, Client>>::Attributes as PayloadBuilderAttributes>::Error: 'static,
{
    type Attributes = Either<L::Attributes, R::Attributes>;
    type BuiltPayload = Either<L::BuiltPayload, R::BuiltPayload>;

    fn try_build(
        &self,
        args: BuildArguments<Pool, Client, Self::Attributes, Self::BuiltPayload>,
    ) -> Result<BuildOutcome<Self::BuiltPayload>, PayloadBuilderError> {
        match args.config.attributes {
            Either::Left(ref left_attr) => {
                let left_args:  BuildArguments<Pool, Client, L::Attributes, L::BuiltPayload> = BuildArguments {
                    client: args.client.clone(),
                    pool: args.pool.clone(),
                    cached_reads: args.cached_reads.clone(),
                    config: PayloadConfig {
                        parent_block: args.config.parent_block.clone(),
                        extra_data: args.config.extra_data.clone(),
                        attributes: left_attr.clone(),
                    },
                    cancel: args.cancel.clone(),
                    best_payload: args.best_payload.clone().and_then(|payload| {
                        if let Either::Left(p) = payload {
                            Some(p)
                        } else {
                            None
                        }
                    }),
                };

                match self.left.try_build(left_args) {
                    Ok(BuildOutcome::Better { payload, cached_reads }) => {
                        return Ok(BuildOutcome::Better {
                            payload: Either::Left(payload),
                            cached_reads,
                        })
                    }
                    Ok(other) => {
                        return Ok(other.map_payload(Either::Left))
                    }
                    Err(_) => {
                    }
                }
            }
            Either::Right(ref right_attr) => {
                let right_args = BuildArguments {
                    client: args.client.clone(),
                    pool: args.pool.clone(),
                    cached_reads: args.cached_reads.clone(),
                    config: PayloadConfig {
                        parent_block: args.config.parent_block.clone(),
                        extra_data: args.config.extra_data.clone(),
                        attributes: right_attr.clone(),
                    },
                    cancel: args.cancel.clone(),
                    best_payload: args.best_payload.clone().and_then(|payload| {
                        if let Either::Right(p) = payload {
                            Some(p)
                        } else {
                            None
                        }
                    }),
                };

                match self.right.try_build(right_args) {
                    Ok(BuildOutcome::Better { payload, cached_reads }) => {
                        return Ok(BuildOutcome::Better {
                            payload: Either::Right(payload),
                            cached_reads,
                        })
                    }
                    Ok(other) => {
                        return Ok(other.map_payload(Either::Right))
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
        }
        Err(PayloadBuilderError::Other(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Both left and right builders failed to build the payload"
        ))))
    }

    fn build_empty_payload(
        &self,
        client: &Client,
        config: PayloadConfig<Self::Attributes>,
    ) -> Result<Self::BuiltPayload, PayloadBuilderError> {
        match config.attributes {
            Either::Left(left_attr) => {
                let left_config = PayloadConfig {
                    attributes: left_attr,
                    parent_block: config.parent_block.clone(),
                    extra_data: config.extra_data.clone(),
                };

                match self.left.build_empty_payload(client, left_config) {
                    Ok(payload_left) => { 
                        return Ok(Either::Left(payload_left))
                    },
                    Err(_) => {}
                }
            },
            Either::Right(right_attr) => {
                let right_config = PayloadConfig {
                    parent_block: config.parent_block.clone(),
                    extra_data: config.extra_data.clone(),
                    attributes: right_attr.clone(),
                };

                match self.right.build_empty_payload(client, right_config) {
                    Ok(payload_right) => { 
                        return Ok(Either::Right(payload_right))
                    },
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
        }
        Err(PayloadBuilderError::Other(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to build empty payload with both left and right builders"
        ))))
    }
}