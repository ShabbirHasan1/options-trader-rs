use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::web_client::WebClient;

use super::web_client::sessions::acc_api;

mod tt_api {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct AccountBalance {
        #[serde(rename = "type")]
        pub type_field: String,
        pub data: AccountData,
        pub timestamp: u64,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct AccountData {
        #[serde(rename = "account-number")]
        pub account_number: String,
        #[serde(rename = "cash-balance")]
        pub cash_balance: String,
        #[serde(rename = "long-equity-value")]
        pub long_equity_value: String,
        #[serde(rename = "short-equity-value")]
        pub short_equity_value: String,
        #[serde(rename = "long-derivative-value")]
        pub long_derivative_value: String,
        #[serde(rename = "short-derivative-value")]
        pub short_derivative_value: String,
        #[serde(rename = "long-futures-value")]
        pub long_futures_value: String,
        #[serde(rename = "short-futures-value")]
        pub short_futures_value: String,
        #[serde(rename = "long-futures-derivative-value")]
        pub long_futures_derivative_value: String,
        #[serde(rename = "short-futures-derivative-value")]
        pub short_futures_derivative_value: String,
        #[serde(rename = "long-margineable-value")]
        pub long_margineable_value: String,
        #[serde(rename = "short-margineable-value")]
        pub short_margineable_value: String,
        #[serde(rename = "margin-equity")]
        pub margin_equity: String,
        #[serde(rename = "equity-buying-power")]
        pub equity_buying_power: String,
        #[serde(rename = "derivative-buying-power")]
        pub derivative_buying_power: String,
        #[serde(rename = "day-trading-buying-power")]
        pub day_trading_buying_power: String,
        #[serde(rename = "futures-margin-requirement")]
        pub futures_margin_requirement: String,
        #[serde(rename = "available-trading-funds")]
        pub available_trading_funds: String,
        #[serde(rename = "maintenance-requirement")]
        pub maintenance_requirement: String,
        #[serde(rename = "maintenance-call-value")]
        pub maintenance_call_value: String,
        #[serde(rename = "reg-t-call-value")]
        pub reg_t_call_value: String,
        #[serde(rename = "day-trading-call-value")]
        pub day_trading_call_value: String,
        #[serde(rename = "day-equity-call-value")]
        pub day_equity_call_value: String,
        #[serde(rename = "net-liquidating-value")]
        pub net_liquidating_value: String,
        #[serde(rename = "cash-available-to-withdraw")]
        pub cash_available_to_withdraw: String,
        #[serde(rename = "day-trade-excess")]
        pub day_trade_excess: String,
        #[serde(rename = "pending-cash")]
        pub pending_cash: String,
        #[serde(rename = "pending-cash-effect")]
        pub pending_cash_effect: String,
        #[serde(rename = "long-cryptocurrency-value")]
        pub long_cryptocurrency_value: String,
        #[serde(rename = "short-cryptocurrency-value")]
        pub short_cryptocurrency_value: String,
        #[serde(rename = "cryptocurrency-margin-requirement")]
        pub cryptocurrency_margin_requirement: String,
        #[serde(rename = "unsettled-cryptocurrency-fiat-amount")]
        pub unsettled_cryptocurrency_fiat_amount: String,
        #[serde(rename = "unsettled-cryptocurrency-fiat-effect")]
        pub unsettled_cryptocurrency_fiat_effect: String,
        #[serde(rename = "closed-loop-available-balance")]
        pub closed_loop_available_balance: String,
        #[serde(rename = "equity-offering-margin-requirement")]
        pub equity_offering_margin_requirement: String,
        #[serde(rename = "long-bond-value")]
        pub long_bond_value: String,
        #[serde(rename = "bond-margin-requirement")]
        pub bond_margin_requirement: String,
        #[serde(rename = "used-derivative-buying-power")]
        pub used_derivative_buying_power: String,
        #[serde(rename = "special-memorandum-account-value")]
        pub special_memorandum_account_value: String,
        #[serde(rename = "special-memorandum-account-apex-adjustment")]
        pub special_memorandum_account_apex_adjustment: String,
        #[serde(rename = "total-settle-balance")]
        pub total_settle_balance: String,
        #[serde(rename = "snapshot-date")]
        pub snapshot_date: String,
        #[serde(rename = "reg-t-margin-requirement")]
        pub reg_t_margin_requirement: String,
        #[serde(rename = "futures-overnight-margin-requirement")]
        pub futures_overnight_margin_requirement: String,
        #[serde(rename = "futures-intraday-margin-requirement")]
        pub futures_intraday_margin_requirement: String,
        #[serde(rename = "maintenance-excess")]
        pub maintenance_excess: String,
        #[serde(rename = "pending-margin-interest")]
        pub pending_margin_interest: String,
        #[serde(rename = "apex-starting-day-margin-equity")]
        pub apex_starting_day_margin_equity: String,
        #[serde(rename = "buying-power-adjustment")]
        pub buying_power_adjustment: String,
        #[serde(rename = "buying-power-adjustment-effect")]
        pub buying_power_adjustment_effect: String,
        #[serde(rename = "effective-cryptocurrency-buying-power")]
        pub effective_cryptocurrency_buying_power: String,
        #[serde(rename = "updated-at")]
        pub updated_at: String,
    }
}

pub struct Account {}

impl Account {
    pub fn new(web_client: Arc<WebClient>, cancel_token: CancellationToken) -> Self {
        let mut receiver = web_client.subscribe_acc_events();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    msg = receiver.recv() => {
                        match msg {
                            Err(RecvError::Lagged(err)) => warn!("Publisher channel skipping a number of messages: {}", err),
                            Err(RecvError::Closed) => {
                                error!("Publisher channel closed");
                                cancel_token.cancel();
                            }
                            std::result::Result::Ok(val) => {
                                Self::handle_msg(val, &cancel_token);
                            }
                        }
                    }
                    _ = cancel_token.cancelled() => {
                        break
                    }
                }
            }
        });
        Self {}
    }

    fn handle_msg(msg: String, _cancel_token: &CancellationToken) {
        if let Ok(payload) = serde_json::from_str::<acc_api::Payload>(&msg) {
            if payload.msg_type.ne("AccountBalance") {
                return;
            }
            if let Ok(msg) = serde_json::from_str::<tt_api::AccountBalance>(&payload.data) {
                info!("Last account balance message received, msg: {:?}", msg);
            }
        }
    }
}
