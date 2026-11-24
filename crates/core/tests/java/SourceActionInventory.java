import com.google.gson.*;
import com.sun.source.tree.*;
import com.sun.source.util.*;
import javax.tools.*;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.*;

/** Parse the read-only Java source without resolving classes or starting the application.
 * Produces an audit inventory, never a claim that any action has been migrated. */
public class SourceActionInventory {
 static String sha(String source)throws Exception {
  return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(source.getBytes(StandardCharsets.UTF_8)));
 }
 public static void main(String[] args)throws Exception {
  var root=Path.of(args[0]).toRealPath();var destination=Path.of(args[1]).toAbsolutePath();
  if(destination.startsWith(root))throw new IllegalArgumentException("Output must stay outside original source");
  List<Path> paths;try(var files=Files.walk(root)){paths=files.filter(p->p.toString().endsWith(".java")).sorted().toList();}
  var diagnostics=new DiagnosticCollector<JavaFileObject>();var compiler=ToolProvider.getSystemJavaCompiler();
  var units=new JsonArray();int[] counts=new int[3];
  try(var manager=compiler.getStandardFileManager(diagnostics,null,StandardCharsets.UTF_8)){
   var task=(JavacTask)compiler.getTask(null,manager,diagnostics,List.of("--enable-preview","--release","25","-proc:none"),null,manager.getJavaFileObjectsFromPaths(paths));
   var positions=Trees.instance(task).getSourcePositions();
   for(var unit:task.parse()){
    var path=Path.of(unit.getSourceFile().toUri());var source=Files.readString(path);
    var file=new JsonObject();file.addProperty("path",root.relativize(path).toString().replace('\\','/'));file.addProperty("sha256",sha(source));
    var methods=new JsonArray();var bindings=new JsonArray();var controls=new JsonArray();
    file.add("methods",methods);file.add("bindings",bindings);file.add("controls",controls);units.add(file);
    new TreeScanner<Void,Void>(){
     String className="",methodName="<initializer>";
     JsonObject location(Tree tree){
      long start=positions.getStartPosition(unit,tree),end=positions.getEndPosition(unit,tree);
      var item=new JsonObject();item.addProperty("class",className);item.addProperty("method",methodName);
      item.addProperty("line",start<0?-1:unit.getLineMap().getLineNumber(start));
      item.addProperty("endLine",end<0?-1:unit.getLineMap().getLineNumber(Math.max(start,end-1)));
      item.addProperty("reviewStatus","unreviewed");return item;
     }
     @Override public Void visitClass(ClassTree node,Void unused){
      var previous=className;className=previous.isEmpty()?node.getSimpleName().toString():previous+"."+node.getSimpleName();
      super.visitClass(node,unused);className=previous;return null;
     }
     @Override public Void visitMethod(MethodTree node,Void unused){
      var previous=methodName;methodName=node.getName().toString();
      var item=location(node);item.addProperty("name",methodName);item.addProperty("parameters",node.getParameters().stream().map(p->p.getType().toString()).toList().toString());
      item.addProperty("hasBody",node.getBody()!=null);methods.add(item);counts[0]++;
      super.visitMethod(node,unused);methodName=previous;return null;
     }
     @Override public Void visitMethodInvocation(MethodInvocationTree node,Void unused){
      String name=node.getMethodSelect() instanceof MemberSelectTree member?member.getIdentifier().toString():node.getMethodSelect().toString();
      if(name.startsWith("setOn")||Set.of("addEventHandler","addEventFilter","setAccelerator","setContextMenu","setOnKeyPressed","setOnMouseClicked").contains(name)||name.equals("put")&&node.getMethodSelect().toString().contains("getAccelerators()")){
       var item=location(node);item.addProperty("binding",node.getMethodSelect().toString());
       var arguments=new JsonArray();for(var argument:node.getArguments()){
        String text=argument.toString();if(text.length()>2048)text=text.substring(0,2048)+"…";arguments.add(text);
       }item.add("arguments",arguments);bindings.add(item);counts[1]++;
      }
      return super.visitMethodInvocation(node,unused);
     }
     @Override public Void visitNewClass(NewClassTree node,Void unused){
      String name=node.getIdentifier().toString();int dot=name.lastIndexOf('.');if(dot>=0)name=name.substring(dot+1);
      if(Set.of("Button","MenuItem","CheckMenuItem","RadioMenuItem","CheckBox","ToggleButton","Tab").contains(name)){
       var item=location(node);item.addProperty("control",name);
       if(!node.getArguments().isEmpty())item.addProperty("firstArgument",node.getArguments().get(0).toString());
       controls.add(item);counts[2]++;
      }
      return super.visitNewClass(node,unused);
     }
    }.scan(unit,null);
   }
  }
  var errors=diagnostics.getDiagnostics().stream().filter(d->d.getKind()==Diagnostic.Kind.ERROR).map(Object::toString).toList();
  if(!errors.isEmpty())throw new IllegalStateException(String.join("\n",errors));
  var result=new JsonObject();result.addProperty("sourceFiles",paths.size());result.addProperty("methods",counts[0]);result.addProperty("bindings",counts[1]);result.addProperty("controls",counts[2]);
  result.addProperty("parseOnly",true);result.addProperty("applicationStarted",false);result.addProperty("configurationWritten",false);result.addProperty("savedHostsContacted",false);result.add("files",units);
  Files.writeString(destination,new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(result));
  System.out.printf("Parsed %d files: %d methods, %d bindings, %d controls; all unreviewed.%n",paths.size(),counts[0],counts[1],counts[2]);
 }
}
