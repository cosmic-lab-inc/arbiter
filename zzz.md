**enter long**
cash = assets.quote
quote = bet / 100.0 * cash
assets.quote -= quote

quote_fee = quote * self.fee / 100.0
quote -= quote_fee

base = quote / price
entry.qty = base
assets.base += entry.qty

self.add_trade(entry)

**exit long**
assets.base -= entry.qty
quote = entry.qty * price

quote_fee = quote * self.fee / 100.0
quote -= quote_fee
assets.quote += quote

exit.qty = entry.qty
self.add_trade(exit)
pct_pnl = (exit.price - entry.price) / entry.price * 100.0

**enter short**
cash = assets.quote
quote = bet / 100.0 * cash
assets.quote -= quote

quote_fee = quote * self.fee / 100.0
quote -= quote_fee

base = quote / price
entry.qty = base
assets.base += entry.qty

self.add_trade(entry)

**exit short**
assets.base -= entry.qty
quote = entry.qty * price

quote_fee = quote * self.fee / 100.0
quote -= quote_fee
assets.quote += quote

exit.qty(entry.qty)
self.add_trade(exit)
pct_pnl = (exit.price - entry.price) / entry.price * 100.0




