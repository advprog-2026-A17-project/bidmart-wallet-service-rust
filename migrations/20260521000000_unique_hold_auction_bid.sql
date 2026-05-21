CREATE UNIQUE INDEX IF NOT EXISTS uq_holds_auction_bid ON holds(auction_id, bid_id);
