use jormungandr_lib::interfaces::*;

use actix_web::error::{Error, ErrorBadRequest, ErrorInternalServerError, ErrorNotFound};
use actix_web::{Error as ActixError, HttpMessage, HttpRequest};
use actix_web::{Json, Path, Query, Responder, State};
use chain_core::property::{Deserialize, Serialize};
use chain_crypto::{Blake2b256, PublicKey};
use chain_impl_mockchain::account::{AccountAlg, Identifier};
use chain_impl_mockchain::key::Hash;

use chain_impl_mockchain::leadership::LeadershipConsensus;
use chain_impl_mockchain::message::Message;
use chain_storage::store;

use bytes::{Bytes, IntoBuf};
use futures::Future;
use std::str::FromStr;

use crate::intercom::TransactionMsg;

pub type Context = crate::rest::Context;

pub fn get_utxos(context: State<Context>) -> impl Responder {
    let blockchain = context.blockchain.lock_read();
    let utxos = blockchain
        .multiverse
        .get(&blockchain.get_tip().unwrap())
        .unwrap()
        .utxos();
    let utxos = utxos.map(UTxOInfo::from).collect::<Vec<_>>();
    Json(utxos)
}

pub fn get_account_state(
    context: State<Context>,
    account_id_hex: Path<String>,
) -> Result<impl Responder, Error> {
    let account_id = parse_account_id(&account_id_hex)?;
    let blockchain = context.blockchain.lock_read();
    let state = blockchain
        .multiverse
        .get(&blockchain.get_tip().unwrap())
        .unwrap()
        .accounts()
        .get_state(&account_id)
        .map_err(|e| ErrorNotFound(e))?;
    Ok(Json(AccountState::from(state)))
}

fn parse_account_id(id_hex: &str) -> Result<Identifier, Error> {
    PublicKey::<AccountAlg>::from_str(id_hex)
        .map(Into::into)
        .map_err(|e| ErrorBadRequest(e))
}

pub fn get_message_logs(context: State<Context>) -> impl Responder {
    let logs = context.logs.lock().unwrap();
    let logs = logs.logs().wait().unwrap();
    Json(logs)
}

pub fn post_message(
    request: &HttpRequest<Context>,
) -> impl Future<Item = impl Responder + 'static, Error = impl Into<ActixError> + 'static> + 'static
{
    let sender = request.state().transaction_task.clone();
    request.body().map(move |message| -> Result<_, ActixError> {
        let msg = Message::deserialize(message.into_buf()).map_err(|e| {
            println!("{}", e);
            ErrorBadRequest(e)
        })?;
        let msg = TransactionMsg::SendTransaction(FragmentOrigin::Rest, vec![msg]);
        sender.lock().unwrap().try_send(msg).unwrap();
        Ok("")
    })
}

pub fn get_tip(settings: State<Context>) -> impl Responder {
    settings
        .blockchain
        .lock_read()
        .get_tip()
        .unwrap()
        .to_string()
}

pub fn get_stats_counter(context: State<Context>) -> impl Responder {
    let stats = &context.stats_counter;
    Json(json!({
        "txRecvCnt": stats.get_tx_recv_cnt(),
        "blockRecvCnt": stats.get_block_recv_cnt(),
        "uptime": stats.get_uptime_sec(),
    }))
}

pub fn get_block_id(
    context: State<Context>,
    block_id_hex: Path<String>,
) -> Result<Bytes, ActixError> {
    let block_id = parse_block_hash(&block_id_hex)?;
    let blockchain = context.blockchain.lock_read();
    let block = blockchain
        .storage
        .read()
        .unwrap()
        .get_block(&block_id)
        .map_err(|e| ErrorBadRequest(e))?
        .0
        .serialize_as_vec()
        .map_err(|e| ErrorInternalServerError(e))?;
    Ok(Bytes::from(block))
}

fn parse_block_hash(hex: &str) -> Result<Hash, ActixError> {
    let hash: Blake2b256 = hex.parse().map_err(|e| ErrorBadRequest(e))?;
    Ok(Hash::from(hash))
}

pub fn get_block_next_id(
    context: State<Context>,
    block_id_hex: Path<String>,
    query_params: Query<QueryParams>,
) -> Result<Bytes, ActixError> {
    let block_id = parse_block_hash(&block_id_hex)?;
    // FIXME
    // POSSIBLE RACE CONDITION OR DEADLOCK!
    // Assuming that during update whole blockchain is write-locked
    // FIXME: don't hog the blockchain lock.
    let blockchain = context.blockchain.lock_read();
    let storage = blockchain.storage.read().unwrap();
    store::iterate_range(&*storage, &block_id, &blockchain.get_tip().unwrap())
        .map_err(|e| ErrorBadRequest(e))?
        .take(query_params.get_count())
        .try_fold(Bytes::new(), |mut bytes, res| {
            let block_info = res.map_err(|e| ErrorInternalServerError(e))?;
            bytes.extend_from_slice(block_info.block_hash.as_ref());
            Ok(bytes)
        })
}

const MAX_COUNT: usize = 100;

#[derive(Deserialize)]
pub struct QueryParams {
    count: Option<usize>,
}

impl QueryParams {
    pub fn get_count(&self) -> usize {
        self.count.unwrap_or(1).min(MAX_COUNT)
    }
}

pub fn get_stake_distribution(context: State<Context>) -> Result<impl Responder, Error> {
    let blockchain = context.blockchain.lock_read();

    // TODO don't access storage layer, but instead get the date somewhere else
    let (block, _) = blockchain
        .get_block_tip()
        .map_err(|e| ErrorInternalServerError(e))?;
    let last_epoch = block.header.block_date().epoch;
    // ******

    let mleadership = blockchain.leaderships.get(last_epoch);
    match mleadership {
        None => Ok(Json(json!({ "epoch": last_epoch }))),
        Some(mut leadership) => match leadership.next().map(|(_, l)| l.consensus()) {
            Some(LeadershipConsensus::GenesisPraos(gp)) => {
                let stake = gp.distribution();
                let pools: Vec<_> = stake
                    .to_pools
                    .iter()
                    .map(|(h, p)| (format!("{}", h), p.total_stake.0))
                    .collect();
                Ok(Json(json!({
                    "epoch": last_epoch,
                    "stake": {
                        "unassigned": stake.unassigned.0,
                        "dangling": stake.dangling.0,
                        "pools": pools,
                    }
                })))
            }
            _ => Ok(Json(json!({ "epoch": last_epoch }))),
        },
    }
}
