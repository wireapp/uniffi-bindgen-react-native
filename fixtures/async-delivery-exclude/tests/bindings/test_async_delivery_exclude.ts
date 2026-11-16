/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/
 */
// To run:
//   cargo test -p uniffi-fixture-async-delivery-exclude -- async_wasm

import {
  Counter,
  Suite,
  SuiteError,
  add,
  echoBytes,
  readGlobal,
  suiteFromU16,
  version,
  versionInfo,
  writeGlobal,
} from "@/generated/uniffi_async_delivery_exclude";
import { asyncTest, test } from "@/asserts";
import "@/polyfills";

const isPromise = (value: unknown): boolean => value instanceof Promise;

(async () => {
  test("excluded functions answer synchronously", (t) => {
    const v = version();
    t.assertFalse(isPromise(v), "version() must not return a promise");
    t.assertEqual(v, "1.2.3");

    const info = versionInfo();
    t.assertFalse(isPromise(info), "versionInfo() must not return a promise");
    t.assertEqual(info.major, 1);
    t.assertEqual(info.label, "one");
    t.assertEqual(info.tags, ["a", "b"]);

    t.assertEqual(suiteFromU16(1), Suite.Small);
    t.assertEqual(suiteFromU16(2), Suite.Large);
  });

  test("an excluded function throws synchronously", (t) => {
    t.assertThrows(
      (e) => SuiteError.Unknown.instanceOf(e) && e.inner.value === 3,
      () => suiteFromU16(3),
    );
  });

  test("an excluded function passes buffers through its own player", (t) => {
    const bytes = new Uint8Array([1, 2, 3, 250]);
    t.assertEqual([...new Uint8Array(echoBytes(bytes.buffer))], [1, 2, 3, 250]);
  });

  await asyncTest("delivered functions and objects still return promises", async (t) => {
    const sum = add(2, 3);
    t.assertTrue(isPromise(sum), "add() must return a promise");
    t.assertEqual(await sum, 5);

    const counter = await Counter.create(41);
    t.assertEqual(await counter.increment(), 42);
    counter.uniffiDestroy();
    t.end();
  });

  await asyncTest("an excluded function runs on a separate instance", async (t) => {
    // The delivered call writes the global behind the port; the excluded one
    // reads the global of the instance on this thread, which never changed.
    await writeGlobal(7);
    t.assertEqual(readGlobal(), 0);
    t.end();
  });
})();
