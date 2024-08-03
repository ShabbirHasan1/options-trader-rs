use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use super::generic::StrategyMeta;
use crate::platform::{mktdata::MktData, orders::Orders, positions::*};

pub struct SpxStrategy {
    mkt_data: Arc<RwLock<MktData>>,
    orders: Arc<RwLock<Orders>>,
    strategy_meta: Option<Arc<dyn StrategyMeta>>,
}

impl SpxStrategy {
    pub fn new(mkt_data: Arc<RwLock<MktData>>, orders: Arc<RwLock<Orders>>) -> Self {
        // Self::market_monitor();
        Self {
            mkt_data,
            orders,
            strategy_meta: None,
        }
    }

    pub async fn enter_position<Meta>(orders: &Arc<RwLock<Orders>>, price_effect: PriceEffect) {
        // if let Err(err) = orders
        //     .write()
        //     .await
        //     .liquidate_position(strategy_meta, price_effect)
        //     .await
        // {
        //     error!("Failed to send order, error: {:?}", err);
        // }
    }

    // fn market_monitor() {
    //     tokio::spawn(async move {
    //         tokio::select! {
    //             _ = sleep(Duration::from_secs(5)) => {
    //                 // if orders.has_symbol() {
    //                 //     return
    //                 // }

    //                 // let diretion = get_market_conditions();
    //                 // if diretion.is_none() {
    //                 //     return
    //                 // }
    //                 // let _ = self.enter_position();
    //             }
    //         }
    //     });
    // }
}

// impl StrategyMeta for SpxStrategy {
//     fn get_underlying(&self) -> &str {
//         &self
//             .position
//             .as_ref()
//             .unwrap()
//             .legs
//             .first()
//             .unwrap()
//             .underlying
//     }

//     fn get_symbols(&self) -> Vec<&str> {
//         self.position
//             .as_ref()
//             .unwrap()
//             .legs
//             .iter()
//             .map(|leg| leg.symbol.as_str())
//             .collect()
//     }

//     fn get_option_type(&self) -> OptionType {
//         self.position
//             .as_ref()
//             .unwrap()
//             .legs
//             .first()
//             .unwrap()
//             .option_type
//     }

//     fn get_position(&self) -> &Position {
//         &self.position.as_ref().unwrap()
//     }
// }
