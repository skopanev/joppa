// The shared state of the board page. Modules read and change these fields through this object.
// ws is the selected workspace, data is its board reply, and loadNumber counts loads, so an
// older reply can see that a newer load replaced it.
export const state={ws:new URLSearchParams(location.search).get('workspace')||'',data:null,viewerActor:'',loadNumber:0};
