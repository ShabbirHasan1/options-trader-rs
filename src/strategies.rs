trait Strategy {
    fn should_exit(&self, mktdata: &MktData) -> bool;
}

struct CreditSpread {
    symbol: String,
    underlying: String,
}

impl Strategy for CreditSpread {
    fn should_exit(&self, mktdata: &MktData) -> bool {
        let snapshot = mktdata.get_snapshot(self.symbol);
        if snapshot.leg[0].delta > leg[0].delta + 15 {
            return true;
        }

        if orders.get_order().premimum % 2 < orders.orders.get_order().pnl {
            return true;
        }

        return false;
    }
}

struct Strategies {}

impl Strategies {
    async fn get_strategies(http_client: &HttpClient, mktdata: &MktData) {}

    pub async fn new(
        http_client: HttpClient,
        mktdata: MktData,
        orders: Orders,
        cancel_token: CancellationToken,
    ) -> Self {
        let strategies = Self::get_strategies(&http_client).await;
        tokio::spawn(async {
            loop {
                tokio::select! {
                    _ = sleep(Duration::from_secs(5)) => {
                        Self::check_stops();
                    }
                    _ = cancel_token.cancelled() => {
                        break
                    }
                }
            }
        });
    }
}
