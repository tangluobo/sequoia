import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.*;
import com.tangluobo.tomato.module.connect.dialog.CopyTableDialog;
import com.tangluobo.tomato.module.connect.service.DatabaseService;
import com.tangluobo.tomato.module.connect.view.SqlEditorPane;
import java.nio.file.*;
import java.lang.reflect.*;
import java.sql.*;
import java.util.*;
import javax.imageio.ImageIO;
/** Scene snapshot only. Original constructors and controls; synthetic JDBC never opens a socket. */
public class CopyTableWindowOracle {
 static Object field(Object value,String name)throws Exception{var f=CopyTableDialog.class.getDeclaredField(name);f.setAccessible(true);return f.get(value);}
 static Object call(Object value,String name)throws Exception{var m=CopyTableDialog.class.getDeclaredMethod(name);m.setAccessible(true);return m.invoke(value);}
 static java.sql.Connection connection(){return DatabaseObjectsWindowOracle.proxy(java.sql.Connection.class,(self,method,args)->switch(method.getName()){
  case "isValid"->true;
  case "createStatement"->DatabaseObjectsWindowOracle.proxy(Statement.class,(s,m,a)->m.getName().equals("executeQuery")?DatabaseObjectsWindowOracle.result(List.of(DatabaseObjectsWindowOracle.row("1","sample_db"),DatabaseObjectsWindowOracle.row("1","sample_target"))):DatabaseObjectsWindowOracle.basic(m));
  default->DatabaseObjectsWindowOracle.basic(method);
 });}
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application{
 @SuppressWarnings({"unchecked","rawtypes"}) public void start(Stage owner)throws Exception{
  var source=new ConnectionConfig();source.setId("transfer-source-fixture");source.setName("本地数据库");source.setType(ConnectType.MYSQL);source.setHost("127.0.0.1");source.setPort(3306);source.setDatabase("sample_db");
  var target=new ConnectionConfig();target.setId("transfer-target-fixture");target.setName("目标数据库");target.setType(ConnectType.MYSQL);target.setHost("127.0.0.1");target.setPort(3306);target.setDatabase("sample_target");
  var cache=DatabaseService.class.getDeclaredField("connectionCache");cache.setAccessible(true);((Map)cache.get(null)).put(source.getId(),connection());((Map)cache.get(null)).put(target.getId(),connection());
  var dialog=new CopyTableDialog(owner,List.of(source,target),source,"sample_db",null,List.of("users"));
  ((ComboBox<String>)field(dialog,"targetConnCombo")).setValue("目标数据库  (127.0.0.1:3306)");
  var root=(BorderPane)field(dialog,"root");
  if(getParameters().getRaw().get(1).equals("1")){
   root.setCenter((Node)call(dialog,"buildStepTwoCenter"));root.setBottom((Node)call(dialog,"buildStepTwoBottom"));
   var tableList=(VBox)field(dialog,"tableListContainer");tableList.getChildren().addAll(new CheckBox("orders"),new CheckBox("users"));((CheckBox)tableList.getChildren().get(1)).setSelected(true);
   ((SqlEditorPane)field(dialog,"sqlPreviewPane")).setText("-- users\nCREATE TABLE `sample_target`.`users` (\n  `id` bigint NOT NULL,\n  `name` varchar(255) DEFAULT NULL,\n  PRIMARY KEY (`id`)\n);\n");
   ((Label)field(dialog,"stepTwoStatusLabel")).setText("1 条 SQL，1 张表");
  }
  var original=(Stage)field(dialog,"dialogStage");original.getScene().setRoot(new BorderPane());original.setScene(null);var scene=new Scene(root,800,600);scene.getStylesheets().add(CopyTableDialog.class.getResource("/css/connect-tree.css").toExternalForm());owner.setScene(scene);owner.setOpacity(.01);owner.show();
  var wait=new PauseTransition(Duration.seconds(2));wait.setOnFinished(event->{try{
   ((ComboBox<String>)field(dialog,"targetDbCombo")).setValue("sample_target");root.applyCss();root.layout();
   var path=Path.of(getParameters().getRaw().get(0));ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",path.toFile());
   var report=new JsonObject();report.addProperty("width",scene.getWidth());report.addProperty("height",scene.getHeight());report.addProperty("syntheticJdbcOnly",true);report.addProperty("socketsOpened",false);report.addProperty("systemClipboardWritten",false);var controls=new JsonArray();DatabaseObjectsWindowOracle.walk(root,controls);report.add("controls",controls);Files.writeString(Path.of(path+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(report));owner.close();Platform.exit();System.exit(0);
  }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
 }
 }
}
