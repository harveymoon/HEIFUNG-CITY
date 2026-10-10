# Why so few bus and metro riders, so much walking, and the traffic complaints

Asked in the playtest list (2026-10-10): "im not seeing a lot of ridership on buses and
metro but a lot of walking and complaining about traffic, can you explain why?" and "some
of my bus lines have no ridership, is this because I do not have enough bus depots?"

Measured on a copy of the day-67 save (Cinder Ridge, 4,800 people) with
`cargo run --profile fast -p nd_sim --example transit_probe -- <save> 240 -modes`, which
prints the lines, the depots and the new `World::ridership_report` (also the remote API's
`report {section: "ridership"}`): every working resident's commute, home door to work door,
weighed the way a traveller weighs it.

## What the save showed

Trips yesterday: bike 12,438, walk 11,954, metro 2,675, bus 1,880, car 859, train 11.

1. **Two of the four bus lines never got a bus.** "Full City" (21 stops) and "Line 6"
   (15 stops) each wanted 5 buses and had 0 out at 09:00. The dispatcher only ever tried
   *the first line short of buses*; when that line's bus could not be sent from the depot
   nearest its first stop, it tried again every tick and every line after it starved.
   Depots were not the problem: four depots, room for 40 buses, 10 out. Fixed (see below):
   after the fix both lines had their buses within a few hours and riders by noon.
2. **People who stood at a stop for a line with no buses learnt to hate the bus.** After
   three hours a rider gives up, walks, and remembers the bus as a bad mode (commute pain),
   which weighs against the bus on every later trip. Lines no bus can be sent to are no
   longer offered to riders.
3. **Most commutes are short and most people have a bike.** The median commute is 500 m.
   For it, a bike takes about 35 minutes door to door; the bus takes the walk to the stop
   and from it (at walking pace, the same 1.4 m/s as any walk), the wait and the ride: 80 to
   90 minutes on average where a bus was offered, against 39 by car and about 100 on foot.
   The bike was the cheapest way for 2,200 of 3,300 commutes, the metro for 700. Buses win
   for longer trips by people without a bike or a car, which is who rides them.
4. **Stops were out of reach.** A rider would only walk 110 m to a stop. With that, 1,024
   commutes had no stop at the home end, 300 none at the work end, 221 neither. Raised to
   160 m (the walk is in the rider's sums either way): bus on offer for 1,408 commutes
   instead of 748.
5. **Lines are one-way loops, so half the trips go the long way round.** For 586 to 725
   commutes a line serves both ends but the ride forward round the loop is more than 2.5x
   the distance, so walking or cycling wins. Draw a line out along one side of a street and
   back along the other (or two lines, one each way) for the return trips.
6. **No changes between lines.** A trip needed one line serving both ends. Riders now
   change once, between stops of two lines within 90 m (170 commutes could not, 86 after).
7. **Every bus ride hurt.** Commute pain counted a bus ride as bad when it took longer than
   four times walking pace door to door plus 20 minutes; with the walks to and from the stop
   no short bus trip could make that, so riders came off every bus hurting. The mark is now
   twice walking pace plus 25 minutes: pain for a long wait or a slow, jammed bus, not for
   the walk to the stop.
8. **A flat 12-minute wait.** The expected wait is now half the line's headway (its loop
   time over its buses): more buses on a line make it more attractive.

## Walking and the traffic complaints

Walking is the cheapest way for most short trips other than commutes (shopping, errands,
parks), and a walk of more than 25 minutes (about 140 m of clock time at the game's speed)
already counts as a bad trip. Long walks are what most "commute" complaints are about; the
traffic complaints come from the drivers (859 car trips) stuck in the jams round the
interchanges, whose remembered pain is what kept many car owners on bikes. Bus and metro
lines that reach both ends of a common trip, with enough buses for a short wait, are what
moves walkers onto transit.

## Levers the player now has

- The Lines window lists each line's problems (no depot, no depot reaching it, more buses
  wanted than the depots hold, a leg with no road or the long way round, night).
- Fares per line (`bus {action: "set_fare"}`, the Lines window's fare buttons): cheaper
  fares draw riders who weigh money heavily.
- Each line's books: riders, fares and its share of the fleet's upkeep, yesterday.
