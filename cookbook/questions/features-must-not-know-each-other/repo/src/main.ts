import { addToCart, total } from './features/cart/cart';
import { charge } from './features/checkout/checkout';

console.log(charge(total(addToCart([], 'sku-1'))));
