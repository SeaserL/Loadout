// SPDX-License-Identifier: Apache-2.0
import { AsyncLocalStorage } from "node:async_hooks";
const globals = globalThis as typeof globalThis & { __loadoutSubscriptionWrite?: AsyncLocalStorage<boolean> };
const scope = globals.__loadoutSubscriptionWrite ??= new AsyncLocalStorage<boolean>();
export const subscriptionWrite = <T>(work: () => Promise<T>) => scope.run(true, work);
export const isSubscriptionWrite = () => scope.getStore() === true;
