- Is there a way to have it so that the type system & compiler automatically check the graph? Like to ensure that every `produces()` is consumed by somethign instead of a runtime graph being built? Does this even make sense? Idk, I feel like maybe we would still need the graph (things like cycle detection?) and the check is an init thing, so the overhead is not much. Idk? 

- I feel like I'm coverging on a declaritve vs imparitive design. Are there any key aspects of this kind of program design I'm missing? I feel pretty good about cache mutation being in one place, the strict story arond ports/commands/events/messages/components and their interaction, etc. It's pretty low level, but I feel like it cna be built upon. Are there any other building blocks I need for a systme like this? Thoughts? Ideas?

- what are you thgoutsh on exmapnding the `.produces()` api? Instead of a generic produces, what about a `must_produce()` and a `may_produce()` variant? Pros? Cons? Impacts throughout the system? thoughts? ideas? 

- Does it make sense for the MVP to only have one way to register something? Whether that be the impl or hte app.x api? Which wone is easier/more idomatic for this pattern? I feel like the app. api? thoughts?

- I'm a bit confused about the `SimPort` and `LivePort` traits. like waht is run with `LivePortIo<P::Command, P::Event>`? And why woudl the be so different than the on_command signature? I need this broken down like I'm a retard

- Do we still have the concept of a reducer in this model? I feel like we dont? Do we need it though? It owuld be like a component, except that it receives a `&mut Cache` and cannot produce commands/messages. It may make sense to only have cache mutations happen here? It could allow down the line to process components in parallel since they would only need a read only cache...?

- We need to wrok out the cache keys and how the cache actually works. I'm open for ideas on implementaiton. the main goal is to just preserve state across the engine (orders, positions, shared state, etc). How this works, idk yet. I had a v3 versin, but idk if that was the right model? 

- Idk if I like the `Timer` port idea? Is this idiomatic? If feel like the idea of ports are that they are *external* to the system. A scheduled message/command *feels* like not external? I could be wrong though? Am I stuck on the only system/designs? Same idea with 16.5. Is this the right model? I guess it makes sense? Like othereise, how would the engien define it's own events? Events are external... idk?

- I think we need a much more developed port story. Like what ones are in their onw thread? Which are in a work pool? Section 12.6 is very very vague on this? Same wiht the lifecycle in 12.7 - it's very out there and abstract. Same wih t12.9 and 12.10. The whole port idea si great and I feel like the righ tanswer, but the semantics/implementatoin needs some seriousl work?? 

- Instead of `.add()` for components, it should be `.component()`

- I feel like the replay story is pretty weak atm. Idk if this is somethig that we should leave for later or work it out now? I want to keep the v1 a MVP, but idk? thoughts? Like it would requrie all new ports that dont emit events? idk it just feels like somethig that sounds good in theory, but it feels bolted on? I suppoe it could be possible? idk?

- How can we do cycle detection? Is this even feasable? I feel like we should hold off on this for now and just rely on a simple metric like `max_invocations_per_round` until we can figure out some sort of good static code analysis

- I think the durability story plays into the replay story. It's just not there yet, all there is is vague ideas/abstract concepts. Should I save this for later also?

- 10.3 - this is aboslute required. Callbacks MUST recieve typed messages/evnts!

- I'm worried a bit about iternal erasure. As long as hte user facing api stays the same and they have a typed api with no downcasting, I suppose it's okay. Worse case, it can change later since the public api would remain the same

- Lets ignore 13.7 for the mvp. It jsut adds extra complication

- I love the idea of 13.8, but I'm nto too worried about it for the MVP

- The binding 14.3 is good. As long as we check that each port in the graph has a binding

- Before implementing, we need to work out the exact context types and public api surface that we commit to maintaining

- The open comment on 20.3 seems like a port specifiv config that's outside the scope of Kavod core?? Same iwth 20.4, the port will load however it wants

- I feel like a full blown DST like 21.1 is talking about is outside the scope of this? The determnsim boundary is the kernel. Ports dont have to be determinsitic. It can be a later goal, but it seems like too much for an MVP. As long as the same events emmited at the same time/order result in the same outputs, i think thats the scope? WE dont need to provide a mecanism for simulating network or storage IO? that seem slike a lot? It's very hard to draw that line because 21.4 are all things for ht emost part that I want to bake in. but idk how to do htis? I've never done something like this before? Is it even possible? Woudlnt *every* dependency need to use my primitives? idk 

- I like the idea of 23.3 as a tracing story, we can figure out 23.5 later, but it will eventaully be part of the egine config builder. 

- I think we need converge on an error story. Do we return errors for invariant breaks or panic? My gut says to just panic and go full tigerstyle and NASA rules of 10 style - if hte assumptions about the world are borke, I must stop immediatly. It's riskier, but with DST and the overall architecture, it feels best. Perhaps later, we can somehow wire up a `PanicHandler` to do things like lcose out positions? Idk. I know it's risky to panci iwth live money, but that's what all robust software does :/. Ports ar eperhaps a different story though, this is part of the vague port story we have, we need to defien lifecycle stuff fo r them. 

- I think we need some sort of engine config. Things like logging socket, max ations (componetn invocations per turn, etc) that we can set. I guess this belongs on the env builders? Idk? thoughts? ideas? SHould it be it's own thing that goes ito run? Should it og on app? 


