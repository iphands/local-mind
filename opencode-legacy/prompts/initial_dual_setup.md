## Goals

Im trying to configure opencode so that I can have two agents helping me code:
- Each agent comes from another "provider" or connection
- The agents are small I need to set expected context windows to be small on each (48k for one and 32k for the other)
- I want the agents to work "together":
  - AgentA will propse a change
  - AgentB will review the change and add suggestions / fixes
  - AgentA will review suggetions and sent back to Agent B
  - Loop there until both agents are mostly in agreement that things are good
  - AgentA implements changes
  
## Where to find info

I want you to use the web to learn things about omo and opencode
I also have both projects cloned into the vendor/ directory you can use this too for learning
  
## Open Questions

### Vanilla opencode vs omo (oh-my-opencode)

I want to know if I should use base opencode or opencode + oh-my-opencode. Right now I have oh-my-opencode installed / setup.
omo already has a multi agent setup that is somewhat close to this...
BUT im not sure if omo is flexible in letting me define my own new multi agent setups.

### Additions of other agents

I really want to be able to add additional agents soon. Its important to me to be able to add new agents to this mix
and control what they do, are responsible for and how they interact.

After I add more its VERY important for me to be able to control as easy as possible when Im operating with:
- My standard two agents (free cost wise for me)
- My two agents + super agent (super agent costs $$)
- My two agents + sr agent (sr agent also costs $$ but less than super agent)

I need to be able to switch rapidly in opencode

## Final

I want you to research on the omo documentation and tell me a plan of attack
