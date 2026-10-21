// The fixture data of tests/browser.cjs: the tree, the rows and the object details that the
// mocked server returns.
const ATTACK='</script><script>window.attacked=true</script>';
const types={
  area:{title:'Area',index:10,role:'group',root:true,parents:[]},
  spec:{title:'Spec',index:20,role:'requirement',root:false,parents:['area']},
  measure:{title:'Measure',index:30,role:'ac',root:false,parents:['spec']},
};
const schema={workspace:'w',current_revision:2,schema:{revision:2,actor:'owner',
  at:'2026-10-06T09:00:00Z',reason:'Configured tree',types}};

const object=(address,extra={})=>({address,kind:address.split(':')[0],id:address.split(':')[1],
  title:extra.title||'Object',revision:1,present:true,owner:'owner',at:'2026-10-06T09:00:00Z',
  work_ready:true,review_required:false,item_id:'w-O-ABCDEFGHIJ',links:[],
  parent:extra.parent??null,...extra});
const rows=[
  object('area:core',{title:'Core area'}),
  object('spec:access',{title:'Access spec',parent:'area:core',process:{req:'access',revision:3}}),
];
const detail=address=>({workspace:'w',address,kind:address.split(':')[0],id:address.split(':')[1],
  item_id:'w-O-ABCDEFGHIJ',current_revision:1,revision_count:1,work_ready:true,review_required:false,
  process:rows.find(r=>r.address===address)?.process||null,
  document:{revision:1,parent:rows.find(r=>r.address===address)?.parent||null,present:true,links:[],
    data:{title:rows.find(r=>r.address===address)?.title||'Object',owner:'owner',body:ATTACK},
    actor:'owner',at:'2026-10-06T09:00:00Z',reason:'Create'},
  links:[],incoming:[],
  annotations:{comments:{},attachments:{f1:{name:'evidence.png',content_type:'image/png',
    size:2048,hash:'abc',actor:'owner',at:'2026-10-06T09:00:00Z',removed:false}}}});

module.exports={ATTACK,types,schema,object,rows,detail};
