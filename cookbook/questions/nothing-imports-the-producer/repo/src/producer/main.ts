import { publishPending, start } from './producer';

const broker = start('kafka://localhost:9092');
setInterval(() => publishPending(broker), 1000);
