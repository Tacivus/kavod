# Thoughts

## Overall
The design doc feels like a bunch of independed peices that try to come together to form a design vs a unified design that is made up of individual pieces. It has some good, but needs major refinement befor eimplementation can begin. 

## General
- WAY too wordy (explainig what it does not do, complex verbage, duplicated words, etc). It's currently almost 35kb. I want the final completed design doc to be <30kb
- Format is awkward. I think invariants should be embed and highlighted in each section as a table or something. They just feel so random at the top
- It's oddly specific on some semantics, yet missing the broad overview rules. For example, random fatal logic is all over the place. Yet there is zero place where fatal handling rules are broadly setup (which woudl eliminte millions of random edge cases throughout the document). THis is just ONE example of this. There are many others that broad simple rules would solve for random edge cases pieced together. 
- Why no enginerring framework section that outlines robust code comes first above everything via the principles in NASA rule of 10, TigerBeetle tiger style, and sqlite3?
- Section ordering/headers feel so random. 


### Individual Sections

#### 5.2
- `panic!()` in Kavod is genually a panic. There are no panic handlers, catching, etc. `panic!()` immediatly crashes the program from the eyes of Kavod and there is nothing defined for what happens

#### 7
- Everything that needs non-stack memory is essentially finite bounded

#### 9 
- WTF is the run gate? 
- What are these "closures"? There are zero closure functions? `on_event()` does not take a closure? 

#### 10 
- More run gate nonsense? 
- There is no `Indeterminate` handoff state. Either it fit into the queue and thus was accepted into the port, or it didnt and a fatal error happens!


#### 12.3
- In general, it may be more useful to just have a list of all log types that are automatcally triggered sync. For example, every message accpeted is synced. Every turn complete is synced. Every command prep is synced. every fatal is synced (or attempted). THat way we dont have to define a billion edge cases for sync. Everything not synced is added tothe qeuue to be synced at the next sync. This is antoher example of oddly specific rules scattered throughout when one simple overarching rule woudl suffice and be mroe clear!

#### 13.3 
- How are these even sent from the host? This seems oddly defined from an MVP. Why nto something simple like - host interrupts are `Fatal(HostInterrupt)` and follow normal fatal seuence. If there is some other shutdown mechanism required, it canbe a port aht feeds an event to grigger graceful shutdown from external source. 

#### 15 
- This section is way too long and wordy. Also, why is it so late? Determinsim is a core claim of Kavod

#### 17 
- THis section si practically useless. THes are mroe scattered invariants that shoudl be embedded!

#### 18
More examples of what Kavod is no (currently). There's infinate thigns that are unsettled. Like what is the exact unicorn event policy?? Waste of space/time!
