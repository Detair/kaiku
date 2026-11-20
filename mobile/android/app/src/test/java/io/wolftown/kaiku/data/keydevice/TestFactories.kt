package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.testutil.FakeBootClock

/** The answer table on a test phone whose running time does not matter to the test. */
fun KeyAnswers(keyDevices: KeyDeviceRepository): KeyAnswers = KeyAnswers(keyDevices, FakeBootClock())

/** The silence notes on a test phone whose running time does not matter to the test. */
fun ServerSilence(keyDevices: KeyDeviceRepository): ServerSilence = ServerSilence(keyDevices, FakeBootClock())
