import { Widget } from './ui/widget';
import { Store } from './ui/store';
import './legacy/plugin';

export const start = (): Widget => new Widget('main', new Store());
