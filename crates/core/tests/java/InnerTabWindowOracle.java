import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import javax.imageio.ImageIO;
import com.google.gson.*;
import java.nio.file.*;

/** Original source CSS and JavaFX skins, inert contents, no sockets or user state. */
public class InnerTabWindowOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
 Stage stage; Path output, resources;
 public void start(Stage stage)throws Exception {
  this.stage=stage;output=Path.of(getParameters().getRaw().get(0));resources=Path.of(getParameters().getRaw().get(1));
  Files.createDirectories(output);capture(0);
 }
 void capture(int index)throws Exception {
  String kind=new String[]{"query","mq","design","query-selected","mq-kafka"}[index];
  var pane=new TabPane();pane.setTabClosingPolicy(TabPane.TabClosingPolicy.UNAVAILABLE);
  boolean mq=index==1||index==4;
  if(!mq)pane.getStyleClass().add(index==2?"design-tab-pane":"query-result-tab-pane");
  String[] labels=index==2?new String[]{"字段","索引","外键","触发器","选项","注释","SQL预览"}:mq?new String[]{"主题","消息"}:new String[]{"信息","结果1","结果2","状态"};
  for(String label:labels)pane.getTabs().add(new Tab(label,new Pane()));
  if(index==3)pane.getSelectionModel().select(1);
  if(mq){
   var nested=new TabPane();nested.setTabClosingPolicy(TabPane.TabClosingPolicy.UNAVAILABLE);
   for(String label:index==4?new String[]{"按时间查询","按Offset查询","发送消息"}:new String[]{"按时间查询","按Message ID查询","按Key查询","发送消息"})nested.getTabs().add(new Tab(label,new Pane()));
   nested.getStylesheets().add(resources.resolve("css/tab-nogap.css").toUri().toString());
   pane.getTabs().get(1).setContent(nested);pane.getSelectionModel().select(1);
   pane.getStylesheets().add(resources.resolve("css/tab-nogap.css").toUri().toString());
  }
  var root=new StackPane(pane);root.getStylesheets().add(resources.resolve("css/connect-tree.css").toUri().toString());
  stage.setScene(new Scene(root,896,140));stage.setOpacity(.01);stage.show();
  var pause=new PauseTransition(Duration.millis(500));pause.setOnFinished(event->{try {
   root.applyCss();root.layout();var path=output.resolve(kind+".png");
   ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",path.toFile());
   var nodes=new JsonArray();
   for(Node node:root.lookupAll(".tab")){
    var b=node.localToScene(node.getLayoutBounds());if(b.getWidth()==0)continue;
    var row=new JsonObject();row.addProperty("x",b.getMinX());row.addProperty("y",b.getMinY());row.addProperty("width",b.getWidth());row.addProperty("height",b.getHeight());row.addProperty("selected",node.getPseudoClassStates().stream().anyMatch(p->p.getPseudoClassName().equals("selected")));
    var label=(Label)node.lookup(".tab-label");row.addProperty("text",label.getText());row.addProperty("font",label.getFont().toString());nodes.add(row);
   }
   Files.writeString(Path.of(path+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(nodes));
   if(index<4)capture(index+1);else{stage.close();Platform.exit();System.exit(0);}
  }catch(Throwable error){error.printStackTrace();System.exit(1);}});pause.play();
 }
}
}
