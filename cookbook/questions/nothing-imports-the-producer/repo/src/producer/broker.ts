export interface Broker {
  send(topic: string, payload: string): void;
}

export function connect(url: string): Broker {
  return {
    send(topic, payload) {
      console.log(`${url} <- ${topic}: ${payload}`);
    },
  };
}
