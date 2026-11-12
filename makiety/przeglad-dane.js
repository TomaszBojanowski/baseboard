// SPDX-License-Identifier: GPL-3.0-or-later

// The readings the Overview of D64 shows, as the test tree of the app has
// them (MS-7E61, CPU sensor of the board at 50 °C): the processor (Tctl),
// the graphics card, the board's system temperature, the warmest memory
// module and drive, the fans that turn.
window.PRZEGLAD = {
  tctlN: 68.2,
  gpuN: 30.0,
  systemN: 37.5,
  nvme0: "28,9",
  dimm2: "30,0",
  // CPU fan and system fans 1–4; the pump and the extra header stand.
  fansTurning: 5,
};
