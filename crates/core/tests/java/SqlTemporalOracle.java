import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.stage.*;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import java.nio.file.*;
import java.time.*;
import java.util.*;
import javax.imageio.ImageIO;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.view.SqlEditorView;
/** Unchanged original temporal methods and owned calendar popup; no DB/clipboard. */
public class SqlTemporalOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage owner)throws Exception {
   var view=new SqlEditorView(List.of(),null,null);var type=Class.forName(SqlEditorView.class.getName()+"$QueryResultEditableTableCell");var ctor=type.getDeclaredConstructors()[0];ctor.setAccessible(true);var cell=ctor.newInstance(view,93,null,0);
   var parse=type.getDeclaredMethod("parseLenientDateTime",String.class);parse.setAccessible(true);var safe=type.getDeclaredMethod("parseSafeInt",String.class);safe.setAccessible(true);var format=type.getDeclaredMethod("formatTemporal",LocalDate.class,Spinner.class,Spinner.class,Spinner.class);format.setAccessible(true);
   var golden=new JsonObject();var times=new JsonArray();
   for(String text:List.of("2024-02-29 12:34:56","2023-02-29 12:34:56","2024-02-30 12:34:56","2024-04-31 12:34:56","2024-13-01 12:34:56","2024-01-32 12:34:56","2024-12-31 24:00:00","2024-12-31 24:00:01","2024-01-01 12:34:56.1","2024-01-01 12:34:56.12","2024-01-01 12:34:56.123456789","2024-01-01T12:34","2024-01-01T12:34:56","2024-01-01T12:34:56.123","2024-01-01T12:34:56Z","2024-01-01T12:34:56+08:00","0000-01-01 00:00:00","0000-01-01T00:00:00","-0001-01-01T00:00","+10000-01-01T00:00","2024-2-1 01:02:03","2024-01-01 1:02:03","2024-01-01 01:2:03","2024-01-01 01:02:3","2024-01-01 01:02","2024-01-01t01:02:03","2024-01-01","","NULL","2024-01-01 23:59:60")) {
    var item=new JsonObject();item.addProperty("input",text);Object value=parse.invoke(cell,text);if(value==null)item.add("output",JsonNull.INSTANCE);else item.addProperty("output",value.toString());times.add(item);
   }golden.add("datetimes",times);
   var numbers=new JsonArray();for(String text:List.of("12","12.9","-12.9","1e3","NaN","Infinity","-Infinity","1e100","-1e100","0x1.8p3"," 12.5 ","12f","12D","１２","abc","","+1.0",".5","0x1p-1")){var item=new JsonObject();item.addProperty("input",text);item.addProperty("output",(Integer)safe.invoke(cell,text));numbers.add(item);}golden.add("integers",numbers);
   var formats=new JsonArray();for(int code:new int[]{91,92,93,2013,2014}){var typed=ctor.newInstance(view,code,null,0);var item=new JsonObject();item.addProperty("type",code);item.addProperty("output",(String)format.invoke(typed,LocalDate.of(2024,2,29),new Spinner<Integer>(0,23,13),new Spinner<Integer>(0,59,4),new Spinner<Integer>(0,59,5)));formats.add(item);}golden.add("formats",formats);
   var output=Path.of(getParameters().getRaw().get(0));Files.createDirectories(output.getParent());Files.writeString(output,new GsonBuilder().serializeNulls().setPrettyPrinting().disableHtmlEscaping().create().toJson(golden));
   int code=Integer.getInteger("tomato.oracle.type",93);var popupCell=ctor.newInstance(view,code,null,0);var input=new TextField(code==91?"2024-02-29":code==92?"12:34:56.75":"2024-02-29 12:34:56.75");var field=type.getDeclaredField("textField");field.setAccessible(true);field.set(popupCell,input);
   owner.setScene(new Scene(new VBox(input),500,100));owner.setOpacity(.01);owner.show();var show=type.getDeclaredMethod("showTemporalPopup");show.setAccessible(true);show.invoke(popupCell);
   var popup=Window.getWindows().stream().filter(window->window instanceof PopupWindow).map(window->(PopupWindow)window).findFirst().orElseThrow();popup.setOpacity(.01);
   var wait=new PauseTransition(Duration.seconds(1));wait.setOnFinished(event->{try{
    var content=((javafx.stage.Popup)popup).getContent().getFirst();content.applyCss();if(content instanceof Parent parent)parent.layout();var path=Path.of(getParameters().getRaw().get(1));ImageIO.write(SwingFXUtils.fromFXImage(content.snapshot(null,null),null),"png",path.toFile());var layout=new JsonObject();layout.addProperty("width",content.getLayoutBounds().getWidth());layout.addProperty("height",content.getLayoutBounds().getHeight());var children=new JsonArray();walk(content,content,children);layout.add("children",children);Files.writeString(Path.of(path+".layout.json"),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(layout));popup.hide();owner.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
 static void walk(Node root,Node node,JsonArray out){if(node instanceof Labeled label||node instanceof Spinner<?>){var bounds=root.sceneToLocal(node.localToScene(node.getLayoutBounds()));var item=new JsonObject();item.addProperty("type",node.getClass().getSimpleName());if(node instanceof Labeled label)item.addProperty("text",label.getText());if(node instanceof Spinner<?> spinner)item.addProperty("value",spinner.getValue().toString());item.addProperty("x",bounds.getMinX());item.addProperty("y",bounds.getMinY());item.addProperty("width",bounds.getWidth());item.addProperty("height",bounds.getHeight());item.addProperty("style",node.getStyle());out.add(item);}if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(root,child,out);}
}
