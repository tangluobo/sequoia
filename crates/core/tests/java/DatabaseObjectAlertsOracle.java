import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.*;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import com.google.gson.*;
import java.nio.file.*;
import java.util.*;
import javax.imageio.ImageIO;
/** Standard OpenJFX alerts with the exact strings/options in original object handlers.
 * Own scenes and button events only. No configuration, database, clipboard or OS input. */
public class DatabaseObjectAlertsOracle {
 record Example(String key,Alert.AlertType kind,String title,String header,String text,String confirm){}
 static List<Example> examples=List.of(
  new Example("drop",Alert.AlertType.CONFIRMATION,"确认删除",null,"确定要删除选中的 2 张表、1 个视图吗？此操作不可恢复！","确认删除"),
  new Example("clear",Alert.AlertType.CONFIRMATION,"清空确认",null,"确定要清空选中的 2 张表的数据吗？\n此操作将删除所有数据（DELETE FROM），不可恢复！","确认清空"),
  new Example("created",Alert.AlertType.INFORMATION,"新建表",null,"表 orders 创建成功",null),
  new Example("rename-error",Alert.AlertType.ERROR,"重命名失败",null,"对象名称已存在",null),
  new Example("validation",Alert.AlertType.WARNING,"新建表","字段设置不完整，请检查以下问题","字段 1: 类型不能为空\n字段 2: 需要指定长度",null),
  new Example("backup-complete",Alert.AlertType.INFORMATION,"备份完成",null,"备份文件已保存:\nC:\\backup\\sample.nb3",null),
  new Example("backup-error",Alert.AlertType.ERROR,"备份失败",null,"错误: 对象名称已存在",null));
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage owner){
   owner.setScene(new Scene(new javafx.scene.layout.Pane(),896,650));owner.setOpacity(.01);owner.show();
   Platform.runLater(()->{try{
    for(var example:examples){
     var alert=new Alert(example.kind());alert.initOwner(owner);alert.setTitle(example.title());alert.setHeaderText(example.header());alert.setContentText(example.text());
     if(example.confirm()!=null)alert.getButtonTypes().setAll(new ButtonType(example.confirm()),ButtonType.CANCEL);
     alert.setOnShown(event->{
      ((Stage)alert.getDialogPane().getScene().getWindow()).setOpacity(.01);
      var pause=new PauseTransition(Duration.millis(500));pause.setOnFinished(done->{try{
       var root=alert.getDialogPane();root.applyCss();root.layout();var path=Path.of(getParameters().getRaw().get(0)+"-"+example.key()+".png");
       ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",path.toFile());
       var result=new JsonObject();result.addProperty("kind",example.key());result.addProperty("width",root.getWidth());result.addProperty("height",root.getHeight());result.addProperty("systemClipboardWritten",false);var controls=new JsonArray();walk(root,controls);result.add("controls",controls);
       var focused=root.getScene().getFocusOwner();result.addProperty("focusClass",focused==null?"":focused.getClass().getSimpleName());if(focused instanceof Labeled label)result.addProperty("focusLabel",label.getText());
       if(getParameters().getRaw().size()>1){
        javafx.event.EventTarget target=focused==null?root.getScene():focused;
        javafx.event.Event.fireEvent(target,new javafx.scene.input.KeyEvent(javafx.scene.input.KeyEvent.KEY_PRESSED,"","",javafx.scene.input.KeyCode.ENTER,false,false,false,false));
        javafx.event.Event.fireEvent(target,new javafx.scene.input.KeyEvent(javafx.scene.input.KeyEvent.KEY_RELEASED,"","",javafx.scene.input.KeyCode.ENTER,false,false,false,false));
        result.addProperty("enterCloses",!alert.isShowing());
       }
       Files.writeString(Path.of(path+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(result));
       if(alert.isShowing()){var cancel=alert.getButtonTypes().stream().filter(type->type.getButtonData().isCancelButton()).findFirst().orElse(alert.getButtonTypes().get(0));((Button)root.lookupButton(cancel)).fire();}
      }catch(Throwable error){error.printStackTrace();System.exit(1);}});pause.play();
     });alert.showAndWait();
    }
    owner.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});
  }
 }
 static void walk(Node node,JsonArray result){
  if(!node.isVisible())return;
  if(node instanceof Control control){
   var bounds=node.localToScene(node.getLayoutBounds());var row=new JsonObject();row.addProperty("class",node.getClass().getSimpleName());
   if(control instanceof Labeled label){row.addProperty("text",label.getText());row.addProperty("fontName",label.getFont().getName());row.addProperty("fontSize",label.getFont().getSize());var nativeFont=com.sun.javafx.scene.text.FontHelper.getNativeFont(label.getFont());if(nativeFont instanceof com.sun.javafx.font.PGFont font)row.addProperty("fontFile",font.getFontResource().getFileName());}
   if(control instanceof Button button){row.addProperty("default",button.isDefaultButton());row.addProperty("cancel",button.isCancelButton());}
   row.addProperty("x",bounds.getMinX());row.addProperty("y",bounds.getMinY());row.addProperty("width",bounds.getWidth());row.addProperty("height",bounds.getHeight());result.add(row);
  }
  if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(child,result);
 }
}
