// Cart pricing helpers used by the checkout page.
export interface LineItem {
  sku: string;
  name: string;
  unitPriceCents: number;
  quantity: number;
}

export interface Cart {
  items: LineItem[];
  currency: string;
  discountPercent?: number;
}

export function formatPrice(cents: number, currency: string): string {
  const amount = (cents / 100).toFixed(2);
  return `${currency} ${amount}`;
}

export function lineTotal(item: LineItem): number {
  return item.unitPriceCents * item.quantity;
}

export function subtotal(cart: Cart): number {
  return cart.items.reduce((sum, item) => sum + lineTotal(item), 0);
}

export function applyDiscount(cents: number, percent: number): number {
  return Math.round(cents * (1 - percent / 100));
}

export function total(cart: Cart): number {
  const sub = subtotal(cart);
  return cart.discountPercent ? applyDiscount(sub, cart.discountPercent) : sub;
}

export function summary(cart: Cart): string[] {
  const lines = cart.items.map(
    (item) => `${item.quantity} x ${item.name}: ${formatPrice(lineTotal(item), cart.currency)}`,
  );
  lines.push(`Subtotal: ${formatPrice(subtotal(cart), cart.currency)}`);
  if (cart.discountPercent) {
    lines.push(`Discount: ${cart.discountPercent}%`);
  }
  lines.push(`Total: ${formatPrice(total(cart), cart.currency)}`);
  return lines;
}

export function isEmpty(cart: Cart): boolean {
  return cart.items.length === 0;
}

export function itemCount(cart: Cart): number {
  return cart.items.reduce((count, item) => count + item.quantity, 0);
}
